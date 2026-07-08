use serde::Deserialize;

#[derive(Deserialize)]
pub struct Status {
    pub status: String,
}

#[derive(Deserialize)]
pub struct Items {
    pub items: Vec<Item>,
}

#[derive(Deserialize)]
pub struct Item {
    pub item_id: u64,
    pub rub_price: i32,
}

#[derive(Deserialize, Debug)]
pub struct ExchangeResponse {
    pub from: FromSection,
}

#[derive(Deserialize, Debug)]
pub struct FromSection {
    pub balance: BalanceDetails,
}

#[derive(Deserialize, Debug)]
pub struct BalanceDetails {
    #[serde(rename = "convertedBalance")]
    pub converted_balance: i32,
}