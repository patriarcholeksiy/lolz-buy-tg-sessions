use std::time::Duration;
use ferogram::{tl, Client};
use ferogram::tl::enums::InputPeer;
use ferogram::tl::functions::folders::EditPeerFolders;
use ferogram::tl::functions::messages::DeleteHistory;
use ferogram::tl::types::InputFolderPeer;
use tracing::{info, warn};
use crate::claimer::tl_utils;
use crate::claimer::tl_utils::get_last_message;

pub async fn purge_mybots(client: &Client, peer: InputPeer) -> anyhow::Result<()> {
    tl_utils::mute_chat(client, peer.clone()).await?;

    if let Err(err) = client.block(peer.clone(), false).await {
        warn!("Не удалось вызывать Unblock для bot father: {err}")
    };
    if let Err(err) = client.send_message(peer.clone(), "/cancel").await {
        warn!("Не удалось отправить первое сообщение /cancel боту: {err}")
    };
    tokio::time::sleep(Duration::from_millis(700)).await;

    let edit_folder = EditPeerFolders {
        folder_peers: vec![tl::enums::InputFolderPeer::InputFolderPeer(InputFolderPeer {
                peer: peer.clone(),
                folder_id: 1,
            })],
    };
    if let Err(err) = client.invoke(&edit_folder).await {
        warn!("Не удалось поместить чат в архив: {err:?}")
    };

    let bots_name = fetch_bot_list(client, peer.clone()).await?;
    info!("Боты к удалению: {bots_name:?}");

    for bot_name in bots_name {
        if let Err(err) = client.send_message(peer.clone(), "/deletebot").await {
            warn!("Не удалось отправить /deletebot: {err}");
            continue;
        }
        tokio::time::sleep(Duration::from_millis(750)).await;

        if let Err(err) = client.send_message(peer.clone(), bot_name.clone()).await {
            warn!("Не удалось отправить имя бота {bot_name}: {err}");
            continue;
        };

        let Ok(message) = get_last_message(client, peer.clone()).await else {
            warn!("Не удалось получить ответ от BotFather для {}", bot_name);
            continue;
        };

        if message.message.contains("Send 'Yes, I am totally sure.'") {
            if client.send_message(peer.clone(), "Yes, I am totally sure.").await.is_err() {
                let _ = client.send_message(peer.clone(), "/cancel").await;
            };
        } else {
            let _ = client.send_message(peer.clone(), "/cancel").await;
        }
        tokio::time::sleep(Duration::from_millis(700)).await;
    }

    if let Err(err) = client.block(peer.clone(), true).await {
        warn!("Не удалось заблокировать bot father: {err:?}")
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
        warn!("Не удалось удалить историю чата у bot father: {err:?}")
    }

    Ok(())
}

async fn fetch_bot_list(client: &Client, peer: InputPeer) -> anyhow::Result<Vec<String>> {
    let mut name_bots = Vec::new();

    client.send_message(peer.clone(), "/mybots").await?;
    let message = get_last_message(client, peer.clone()).await?;

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