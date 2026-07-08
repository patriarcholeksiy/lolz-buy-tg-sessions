use crate::buyer::model::ExchangeResponse;
use reqwest::Client;
use std::time::Duration;
use tracing::warn;

pub async fn get_balance(client: &Client) -> Option<i32> {
    let url = "https://prod-api.lzt.market/balance/exchange";

    let response = match client.get(url).timeout(Duration::from_secs(60)).send().await {
        Ok(response) => response,
        Err(err) => {
            warn!("Ошибка получения баланса: {err:?}");
            return None;
        }
    };

    if !response.status().is_success() {
        warn!("API вернул статус ошибки: {}", response.status());
        return None;
    }

    match response.json::<ExchangeResponse>().await {
        Ok(data) => {
            Some(data.from.balance.converted_balance)
        }
        Err(err) => {
            warn!("Ошибка распарса ExchangeResponse: {err:?}");
            None
        }
    }
}