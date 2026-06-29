use crate::db::{get_all_account_ids, get_telegram_app_by_id};
use crate::models::OrdersResponse;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::Client;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::time::Duration;
use anyhow::Context;
use tracing::{error, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, EnvFilter};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod models;
mod db;
mod claimer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let _guard = init_tracing();

    let db_url = env::var("DATABASE_URL")?;
    let pg_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&*db_url)
        .await?;

    let acc_ids = get_all_account_ids(&pg_pool).await?;

    let mut tg_apps = vec![];

    tg_apps.push(get_telegram_app_by_id(&pg_pool, 1)
        .await
        .context("Не удалось найти конфигурацию TelegramApp в таблице telegram_apps")?);

    tg_apps.push(get_telegram_app_by_id(&pg_pool, 2)
        .await
        .context("Не удалось найти конфигурацию TelegramApp в таблице telegram_apps")?);

    let client = init_client()?;
    let url = "https://prod-api.lzt.market/user/orders?category_id=24&order_by=pdate_to_down";

    let mut timer = tokio::time::interval(Duration::from_secs(15));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    info!("📥 Инициализация: загрузка текущих ордеров с маркета...");
    match client.get(url).send().await {
        Ok(response) => {
            match response.json::<OrdersResponse>().await {
                Ok(parsed_data) => {
                    let mut cache = acc_ids.lock().await;
                    let mut skipped_count = 0;

                    for order in parsed_data.items {
                        if order.category_id == 24 {
                            if cache.insert(order.telegram_id) {
                                skipped_count += 1;
                            }
                        }
                    }
                    info!("✅ Кэш успешно проинициализирован. Пропущено старых ордеров: {}", skipped_count);
                }
                Err(err) => {
                    error!("Ошибка парсинга: {err:?}")
                }
            }
        }
        Err(err) => {
            error!("⚠️ Не удалось сделать первичный запрос для кэша: {err:#?}. Начинаем без него.");
        }
    }

    let mut tg_count: usize = 0;
    loop {
        timer.tick().await;
        info!("Сканирование на новые аккаунты");

        let response = match client.get(url).send().await {
            Ok(res) => res,
            Err(err) => {
                error!("Сетевая ошибка при запросе к маркету: {err:#?}");
                continue
            }
        };

        let parsed_data = match response.json::<OrdersResponse>().await {
            Ok(data) => data,
            Err(err) => {
                error!("Ошибка десериализации JSON ответа Lolzteam: {err:#?}");
                continue
            }
        };

        for order in parsed_data.items {
            if order.category_id != 24 { continue; }

            {
                let cache = acc_ids.lock().await;
                if cache.contains(&order.telegram_id) {
                    continue;
                }
            }
            info!(
                "✨ Найдена новая покупка! \nPhone: {}\nTg ID: {}\nSeller: {}",
                order.telegram_phone, order.telegram_id, order.seller.username
            );

            let tg_index = tg_count % tg_apps.len();
            match claimer::process_order_to_session(&pg_pool, &order, &tg_apps[tg_index].clone()).await {
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

            let mut cache = acc_ids.lock().await;
            cache.insert(order.telegram_id);
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