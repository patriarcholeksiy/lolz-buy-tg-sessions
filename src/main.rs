use crate::models::OrdersResponse;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::Client;
use std::env;

mod models;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let client = init_client()?;

    let url = "https://prod-api.lzt.market/user/orders";

    let response = client
        .get(url)
        .send()
        .await?;

    response.error_for_status_ref()?;

    let parsed_data = response.json::<OrdersResponse>().await?;

    println!("{parsed_data:#?}");

    Ok(())
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
