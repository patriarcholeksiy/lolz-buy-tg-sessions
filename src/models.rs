use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct OrdersResponse {
    pub items: Vec<Order>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Order {
    pub item_id: u64,
    pub item_state: String,
    pub category_id: u32,
    pub published_date: u64,
    pub price: i32,

    pub telegram_item_id: i64,
    pub telegram_id: i64,
    pub telegram_dc_id: String,
    pub telegram_phone: String,
    pub telegram_country: String,
    pub telegram_last_seen: i64,
    pub telegram_username: String,

    #[serde(rename = "loginData")]
    pub login_data: LoginData,

    #[serde(rename = "telegramBots", default)]
    pub telegram_bots: Vec<TelegramBot>,

    pub seller: Seller,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct LoginData {
    pub raw: String,
    #[serde(rename = "encodedRaw")]
    pub encoded_raw: String,
    pub login: String,
    pub password: String,
    #[serde(rename = "encodedPassword")]
    pub encoded_password: String,
    #[serde(rename = "oldPassword")]
    pub old_password: String,
    #[serde(rename = "encodedOldPassword")]
    pub encoded_old_password: Option<String>,
    #[serde(rename = "extraPasswords")]
    pub extra_passwords: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TelegramBot {
    pub id: String,
    pub name: String,
    pub username: Option<String>,
    pub bot_active_user: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct Seller {
    pub user_id: i64,
    pub username: String,
}