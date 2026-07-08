use crate::buyer::model::Items;
use anyhow::Context;
use reqwest::Client;
use std::time::Duration;

pub async fn poll(client: &Client) -> anyhow::Result<Items> {
    let url = "https://prod-api.lzt.market/telegram?origin[]=stealer&origin[]=personal&origin[]=resale&origin[]=autoreg&origin[]=self_registration&dc_id[]=2&dc_id[]=4&dig_max=9&password=no&spam=no&order_by=price_to_up&pmax=25";

    let response = client.get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Сетевая ошибка при запросе к маркету")?;

    let items = response.json::<Items>()
        .await
        .context("Ошибка десериализации JSON ответа от маркета")?;

    Ok(items)
}

