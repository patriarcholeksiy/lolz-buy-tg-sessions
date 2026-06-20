use crate::db::{get_all_account_ids, get_telegram_app_by_id};
use crate::models::OrdersResponse;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::Client;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::time::Duration;
use anyhow::Context;
use crate::services::process_order_to_session;

mod models;
mod db;
mod services;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let db_url = env::var("DATABASE_URL")?;
    let pg_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&*db_url)
        .await?;

    let acc_ids = get_all_account_ids(&pg_pool).await?;

    let telegram_app = get_telegram_app_by_id(&pg_pool, 1)
        .await
        .context("Не удалось найти конфигурацию TelegramApp в таблице telegram_apps")?;

    let client = init_client()?;
    let url = "https://prod-api.lzt.market/user/orders?category_id=24&order_by=pdate_to_down";

    let mut timer = tokio::time::interval(Duration::from_secs(15));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    println!("📥 Инициализация: загрузка текущих ордеров с маркета...");
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
                    println!("✅ Кэш успешно проинициализирован. Пропущено старых ордеров: {}", skipped_count);
                }
                Err(err) => {
                    println!("Ошибка парсинга: {err:?}")
                }
            }
        }
        Err(err) => {
            eprintln!("⚠️ Не удалось сделать первичный запрос для кэша: {err:#?}. Начинаем без него.");
        }
    }

    loop {
        timer.tick().await;
        println!("Сканирование на новые аккаунты");

        let response = match client.get(url).send().await {
            Ok(res) => res,
            Err(err) => {
                eprintln!("Сетевая ошибка при запросе к маркету: {err:#?}");
                continue
            }
        };

        let parsed_data = match response.json::<OrdersResponse>().await {
            Ok(data) => data,
            Err(err) => {
                eprintln!("Ошибка десериализации JSON ответа Lolzteam: {err:#?}");
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
            println!(
                "✨ Найдена новая покупка! \nPhone: {}\nTg ID: {}\nSeller: {}",
                order.telegram_phone, order.telegram_id, order.seller.username
            );

            match process_order_to_session(&pg_pool, &order, &telegram_app.clone()).await {
                Ok(_) => {
                    println!("✅ Успешно обработан рабочий аккаунт: {}", order.telegram_id);
                }
                Err(err) => {
                    // Ошибка вернется как при сетевом сбое (ретраи кончились), так и при сохранении 401/Frozen.
                    // В любом случае мы логируем ее, но ID добавляем в кэш, чтобы не мучать воркер на следующем тике.
                    eprintln!("ℹ️ Завершена обработка аккаунта {} (записан статус или ошибка): {:?}", order.telegram_id, err);
                }
            }

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
