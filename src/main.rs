use crate::db::get_telegram_app_by_id;
use anyhow::Context;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::Client;
use sqlx::postgres::PgPoolOptions;
use std::collections::HashSet;
use std::env;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

mod models;
mod db;
mod claimer;
mod buyer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let _guard = init_tracing();

    let db_url = env::var("DATABASE_URL")?;
    let pg_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&db_url)
        .await?;

    let mut tg_apps = vec![];

    tg_apps.push(get_telegram_app_by_id(&pg_pool, 1)
        .await
        .context("Не удалось найти конфигурацию TelegramApp в таблице telegram_apps")?);

    tg_apps.push(get_telegram_app_by_id(&pg_pool, 2)
        .await
        .context("Не удалось найти конфигурацию TelegramApp в таблице telegram_apps")?);

    let client = init_client()?;

    let current_balance = Arc::new(AtomicI32::new(0));

    let client_clone = client.clone();
    let balance_clone = Arc::clone(&current_balance);
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_mins(60));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            timer.tick().await;
            info!("Обновление баланса...");
            let response_balance = buyer::balance::get_balance(&client_clone).await;

            match response_balance {
                None => warn!("Balance равен None"),
                Some(balance) => {
                    info!("Полученный баланс аккаунта: {balance}");
                    balance_clone.store(balance, Ordering::Relaxed);
                }
            }
        }
    });

    let (order_tx, mut order_rx) = tokio::sync::mpsc::channel(100);

    tokio::spawn(async move {
        let mut tg_count: usize = 0;

        while let Some(order) = order_rx.recv().await {
            let tg_index = tg_count % tg_apps.len();
            match claimer::process_order_to_session(&pg_pool, &order, &tg_apps[tg_index]).await {
                Ok(_) => {
                    info!("✅ Успешно обработан рабочий аккаунт: {}", order.telegram_id);
                }
                Err(err) => {
                    // Ошибка вернется как при сетевом сбое (ретраи кончились), так и при сохранении 401/Frozen.
                    // В любом случае мы логируем ее, но ID добавляем в кэш, чтобы не мучать воркер на следующем тике.
                    error!("ℹ️ Завершена обработка аккаунта {} (записан статус или ошибка): {:?}", order.telegram_id, err);
                }
            }
            tg_count += 1;
        }
    });

    let mut item_ids = HashSet::with_capacity(1001);

    let mut timer = tokio::time::interval(Duration::from_secs(3));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    tokio::time::sleep(Duration::from_secs(10)).await;
    loop {
        timer.tick().await;
        println!("Start poll...");

        let items = match buyer::market::poll(&client).await {
            Ok(items) => items,
            Err(err) => {
                warn!("Во время опроса маркета возникла ошибка: {err:?}");
                continue;
            }
        };
        
        if item_ids.len() > 1000 {
            item_ids.clear();
        }

        for item in items.items {
            if item_ids.insert(item.item_id) {

                let client_cl = client.clone();
                let order_tx_cl = order_tx.clone();

                tokio::spawn(async move {
                    if buyer::purchasing::check_account(&client_cl, item.item_id).await {
                        match buyer::purchasing::confirm_buy(&client_cl, item.item_id, item.rub_price).await {
                            Ok(order) => {
                                info!("✨ Найдена новая покупка! \nPhone: {}\nTg ID: {}\nSeller: {}",
                               order.telegram_phone, order.telegram_id, order.seller.username);
                                if let Err(err) = order_tx_cl.send(order).await {
                                    error!("Критическая ошибка: канал обработчика упал! {:?}", err);
                                }
                            },
                            Err(err) => error!("Ошибка confirm buy\n{err:?}")
                        };

                    }
                });
            }
        }
    }
}

fn init_client() -> anyhow::Result<Client> {
    let token = env::var("LOLZ_TOKEN")?;

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

    let mut auth_value = HeaderValue::from_str(&format!("Bearer {token}"))?;
    auth_value.set_sensitive(true);
    headers.insert(AUTHORIZATION, auth_value);

    Ok(Client::builder().default_headers(headers).build()?)
}


fn init_tracing() -> WorkerGuard {
    let file_appender = tracing_appender::rolling::daily("logs", "lolz.log");
    let (non_blocking_writer, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::from_default_env()
        .add_directive(tracing::Level::INFO.into());

    let console_layer = fmt::layer().with_target(false);

    let file_layer = fmt::layer()
        .with_writer(non_blocking_writer)
        .with_ansi(false)
        .with_target(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    _guard
}