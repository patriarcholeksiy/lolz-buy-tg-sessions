use crate::db::{get_or_create_seller_id, save_account, save_purchase, Account, AccountStatus, TelegramApp};
use crate::models::Order;
use anyhow::{bail, Context};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ferogram::tl::enums::contacts::ResolvedPeer;
use ferogram::tl::enums::{InputPeer, User};
use ferogram::tl::functions::contacts::ResolveUsername;
use ferogram::tl::functions::folders::EditPeerFolders;
use ferogram::tl::functions::messages::DeleteHistory;
use ferogram::tl::types::{InputFolderPeer, InputPeerNotifySettings, InputPeerUser};
use ferogram::{tl, BuilderError, Client, InvocationError, NoRetries, OverflowStrategy};
use sqlx::PgPool;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

const BOT_FATHER_ID: i64 = 93372553;

pub async fn process_order_to_session(pg_pool: &PgPool, order: &Order, telegram_app: &TelegramApp)
                                      -> anyhow::Result<()> {

    let (auth_key, dc_id) = match order.login_data.raw.split_once(':') {
        Some((key, id)) => {
            let parsed_id = id.parse::<u8>().unwrap_or(2);
            (key.to_string(), parsed_id)
        }
        None => {
            let parsed_id = order.telegram_dc_id.parse::<u8>().unwrap_or(2);
            (order.login_data.login.clone(), parsed_id)
        }
    };

    let ip = match dc_id {
        1 => Ipv4Addr::new(149, 154, 175, 53),
        2 => Ipv4Addr::new(149, 154, 167, 51),
        3 => Ipv4Addr::new(149, 154, 175, 100),
        4 => Ipv4Addr::new(149, 154, 167, 91),
        5 => Ipv4Addr::new(91, 108, 56, 165),
        _ => {
            eprintln!("Неизвестный dc_id: {dc_id}");
            bail!("Неизвестный dc_id: {dc_id}")
        }
    };

    let port: u16 = 443;
    let version_v2: u8 = 2;
    let ip_type_byte: u8 = 4;

    let auth_key_bytes = match hex::decode(auth_key) {
        Ok(bytes) => {
            if bytes.len() != 256 {
                eprintln!("Ошибка: ключ для {} не 256 байт", order.telegram_phone);
                bail!("Ошибка: ключ для {} не 256 байт", order.telegram_phone)
            }
            bytes
        }
        Err(err) => {
            bail!("Ошибка декодирования: {err:#?}")
        }
    };

    let mut buffer = Vec::with_capacity(1 + 1 + 1 + 4 + 2 + 8 + 256);

    buffer.push(version_v2);                               // Префикс версии V2 (1 байт)
    buffer.push(dc_id);                            // DC_ID (1 байт)
    buffer.push(ip_type_byte);                                      // Маркер IPv4 (1 байт)
    buffer.extend_from_slice(&ip.octets().to_vec());       // IP-адрес (4 байта)
    buffer.extend_from_slice(&port.to_be_bytes());         // Порт (2 байта Big Endian)
    buffer.extend_from_slice(&(order.telegram_id.to_be_bytes())); // User ID (8 байт Big Endian)
    buffer.extend_from_slice(&auth_key_bytes);

    let string_session = URL_SAFE_NO_PAD.encode(&buffer);

    const MAX_RETRIES: usize = 3;
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    let mut attempts = 0;
    let client = loop {
        attempts += 1;

        match connect_client(&string_session, telegram_app).await {
            Ok(client) => {
                let mut tx = pg_pool.begin().await.context("Не удалось начать транзакцию")?;

                let account = Account {
                    app_id: telegram_app.id,
                    dc_id: dc_id as i32,
                    phone: if order.telegram_phone.is_empty() { None } else { Some(order.telegram_phone.clone()) },
                    session: string_session.clone(),
                    status: AccountStatus::Active,
                    tg_id: order.telegram_id,
                };

                // Передаем &mut *tx вместо просто пула, чтобы выполнять запросы строго внутри одной транзакции
                let acc_id = save_account(&mut *tx, account).await
                    .context("Не удалось сохранить активный аккаунт")?;

                let seller_id = get_or_create_seller_id(&mut *tx, order.seller.user_id, &order.seller.username).await
                    .context("Не удалось обработать продавца")?;

                save_purchase(&mut *tx, acc_id, seller_id, order.price).await
                    .context("Не удалось сохранить покупку")?;

                // Фиксируем изменения в базе данных атомарно!
                tx.commit().await.context("Не удалось закоммитить транзакцию активного аккаунта")?;

                break client;
            }

            Err(BuilderError::Connect(InvocationError::Rpc(rpc_err))) => {
                if rpc_err.code == 401 || rpc_err.name == "FROZEN_METHOD_INVALID" {
                    let mut tx = pg_pool.begin().await.context("Не удалось начать транзакцию")?;

                    let acc_id = save_deactivated_account(
                        &mut *tx,
                        order,
                        telegram_app.id,
                        dc_id as i32,
                        &string_session,
                        rpc_err.code
                    ).await.context("Error when save deactivated acc")?;

                    let seller_id = get_or_create_seller_id(&mut *tx, order.seller.user_id, &order.seller.username).await?;

                    save_purchase(&mut *tx, acc_id, seller_id, order.price)
                        .await
                        .context("Error when save account purchase")?;

                    tx.commit().await.context("Не удалось закоммитить транзакцию невалидного аккаунта")?;

                    println!("Deactivated account {} saved successfully", order.telegram_phone);
                    bail!("Аккаунт деактивирован Телеграмом: код {}", rpc_err.code);
                }

                eprintln!("Временная RPC ошибка (попытка {}/{}): {}", attempts, MAX_RETRIES, rpc_err);
            }

            Err(e) => {
                eprintln!("Сетевой сбой транспорта (попытка {}/{}): {e:#?}", attempts, MAX_RETRIES);
            }
        }

        if attempts >= MAX_RETRIES {
            bail!("Не удалось подключить клиент после {} попыток из-за сетевых проблем", MAX_RETRIES);
        }

        tokio::time::sleep(RETRY_DELAY).await;
    };

    let peer_bot_father = generate_input_peer(&client).await.context("Ошибка генерации peer с bot father")?;
    
    process_bot_father(&client, peer_bot_father).await?;

    Ok(())
}

async fn connect_client(string_session: &String, telegram_app: &TelegramApp) -> Result<Client, BuilderError> {
    let (client, _) = Client::builder()
        .api_id(telegram_app.api_id)
        .api_hash(telegram_app.api_hash.clone())
        .update_queue_capacity(0)
        .session_string(string_session)
        .update_overflow_strategy(OverflowStrategy::DropNewest)
        .retry_policy(Arc::new(NoRetries))
        .system_lang_code("en")
        .device_model("Android")
        .app_version("3.4.5")
        .system_version("16")
        .connect().await?;
    Ok(client)
}

async fn save_deactivated_account(
    executor: &mut sqlx::PgConnection,
    order: &Order,
    app_id: i32,
    dc_id: i32,
    string_session: &str,
    err_code: i32
) -> anyhow::Result<i32> {
    let status = match err_code {
        401 => AccountStatus::Unauthorized,
        _ => AccountStatus::Frozen
    };
    let account = Account {
        app_id,
        dc_id,
        phone: if order.telegram_phone.is_empty() { None } else { Some(order.telegram_phone.clone()) },
        session: string_session.to_string(),
        status,
        tg_id: order.telegram_id,
    };
    let acc_id = save_account(executor, account).await?;
    Ok(acc_id)
}

async fn process_bot_father(client: &Client, peer: InputPeer) -> anyhow::Result<()> {

    let set_settings = tl::enums::InputPeerNotifySettings::InputPeerNotifySettings(InputPeerNotifySettings {
        show_previews: None,
        silent: None,
        mute_until: Some(i32::MAX),
        sound: None,
        stories_muted: None,
        stories_hide_sender: None,
        stories_sound: None,
    });

    if let Err(err) = client.update_notify_settings(peer.clone(), set_settings).await {
        println!("Не удалось обновить настройки оповещения у bot father: {err:?}")
    };

    client.block(peer.clone(), false).await?;

    if let Err(err) = client.send_message(peer.clone(), "/cancel").await {
        println!("Не удалось отправить первое сообщение /cancel боту: {err}")
    };

    tokio::time::sleep(Duration::from_millis(700)).await;

    let edit_folder = EditPeerFolders {
        folder_peers: vec![
            tl::enums::InputFolderPeer::InputFolderPeer(InputFolderPeer {
                peer: peer.clone(),
                folder_id: 1,
            })
        ],
    };
    if let Err(err) = client.invoke(&edit_folder).await {
        println!("Не удалось поместить чат в архив: {err:?}")
    };

    let name_bots = get_name_bots(&client, peer.clone()).await?;
    println!("{name_bots:?}");

    for name_bot in name_bots {
        if let Err(err) = client.send_message(peer.clone(), "/deletebot").await {
            eprintln!("Не удалось отправить /deletebot: {err}");
            continue;
        }

        tokio::time::sleep(Duration::from_millis(750)).await;

        if let Err(err) = client.send_message(peer.clone(), name_bot.clone()).await {
            eprintln!("Не удалось отправить имя бота {name_bot}: {err}");
            continue;
        };

        let Ok(message) = get_last_message(&client, peer.clone()).await else {
            eprintln!("Не удалось получить ответ от BotFather для {}", name_bot);
            continue;
        };

        if message.message.contains("Send 'Yes, I am totally sure.'") {
            if let Err(_) = client.send_message(peer.clone(), "Yes, I am totally sure.").await {
                let _ = client.send_message(peer.clone(), "/cancel").await;
            };
        } else {
            let _ = client.send_message(peer.clone(), "/cancel").await;
        }
        tokio::time::sleep(Duration::from_millis(700)).await;
    }

    if let Err(err) = client.block(peer.clone(), true).await {
        println!("Не удалось заблокировать bot father: {err:?}")
    };
    tokio::time::sleep(Duration::from_millis(500)).await;

    if let Err(err) = client.invoke(&DeleteHistory {
        just_clear: false,
        revoke: true,
        peer: peer.clone(),
        max_id: 0,
        min_date: None,
        max_date: None,
    }).await {
        println!("Не удалось удалить историю чата у bot father: {err:?}")
    }

    Ok(())
}

async fn generate_input_peer(client: &Client) -> anyhow::Result<InputPeer> {
    let ResolvedPeer::ResolvedPeer(peer) = client.invoke(&ResolveUsername {
        username: "BotFather".to_string(),
        referer: None,
    }).await?;
    let User::User(user) = peer.users.first().context("doesn't have user buy username")? else {
        bail!("Peer user have another enum type")
    };
    if user.id != BOT_FATHER_ID {
        bail!("Peer user id is not equal to the declared id")
    }
    Ok(InputPeer::User(InputPeerUser {
        user_id: user.id,
        access_hash: user.access_hash.context("access_hash is None")?,
    }))
}

async fn get_last_message(client: &Client, peer: InputPeer) -> anyhow::Result<tl::types::Message> {
    tokio::time::sleep(Duration::from_millis(500)).await;
    let history = client.invoke(&tl::functions::messages::GetHistory {
        peer,
        offset_id: 0,
        offset_date: 0,
        add_offset: 0,
        limit: 1,
        max_id: 0,
        min_id: 0,
        hash: 0,
    }).await?;

    // println!("{history:#?}");

    let tl::enums::messages::Messages::Slice(history) = history else { bail!("Can't parse last message") };
    let tl::enums::Message::Message(message) = history
        .messages
        .into_iter()
        .next()
        .context("Can't find first mes from bot father")?
    else { bail!("Messages != type tl::types::Message") };
    Ok(message)
}

async fn get_name_bots(client: &Client, peer: InputPeer) -> anyhow::Result<Vec<String>> {
    let mut name_bots = Vec::new();

    client.send_message(peer.clone(), "/mybots").await?;
    let message = get_last_message(&client, peer.clone()).await?;

    if message.message.contains("no bots") {
        return Ok(name_bots)
    }
    if let Some(markup) = &message.reply_markup {
        if let tl::enums::ReplyMarkup::ReplyInlineMarkup(inline_markup) = markup {

            for row in &inline_markup.rows {
                let tl::enums::KeyboardButtonRow::KeyboardButtonRow(row) = row;

                for button in &row.buttons {
                    if let tl::enums::KeyboardButton::Callback(button) = button {
                        let button_text = button.text.clone();
                        name_bots.push(button_text);
                    }
                }
            }
        }
    }

    Ok(name_bots)
}