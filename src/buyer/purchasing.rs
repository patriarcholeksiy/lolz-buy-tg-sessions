use crate::models::Order;
use crate::buyer::model::Status;
use anyhow::bail;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tracing::{error, info, warn};

pub async fn check_account(client: &Client, item_id: u64) -> bool {
    let url = format!("https://prod-api.lzt.market/{item_id}/check-account");

    for attempt in 0..15 {
        let response = match client.post(&url).timeout(Duration::from_secs(45)).send().await {
            Ok(response) => response,
            Err(err) => {
                warn!("Failed to get response for check account item_id {item_id}: {err:?}");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let body_text = match response.text().await {
            Ok(text) => text,
            Err(err) => {
                warn!("Попытка {attempt}: Не удалось прочитать тело ответа для item_id {item_id}: {err:?}");
                continue;
            }
        };

        if body_text.contains("retry_request") {
            warn!("Попытка {attempt}: API просит повторить запрос (retry_request) для item_id {item_id}");
            tokio::time::sleep(Duration::from_secs(3)).await; // Обязательно ждем перед повтором
            continue;
        }



        match serde_json::from_str::<Status>(&body_text) {
            Ok(status) => {
                return if status.status == "ok" {
                    info!("Аккаунт {item_id} прошел проверку");
                    true
                } else {
                    warn!("Неожиданный статус для item_id {item_id}: {}", status.status);
                    false
                }
            }
            Err(err) => {
                warn!("Попытка {attempt}: Ошибка парсинга JSON для item_id {item_id}. Тело ответа: {body_text}. Ошибка: {err:?}");
                break;
            }
        }
    }

    error!("Не удалось проверить аккаунт {item_id} (исчерпаны попытки или получена фатальная ошибка)");
    false
}

pub async fn confirm_buy(client: &Client, item_id: u64, price: i32) -> anyhow::Result<Order> {
    let response = client.post(format!("https://prod-api.lzt.market/{item_id}/confirm-buy"))
        .json(&json!({
            "price": price,
        }))
        .send()
        .await?;

    if response.status().is_success() {
        let order = response.json::<Order>().await?;
        info!("Аккаунт {item_id} успешно куплен за {price}");
        Ok(order)
    } else {
        let status = response.status();
        let error_body = response.text().await?;

        bail!("API lolzteam вернуло ошибку {status}: {error_body}")
    }
}