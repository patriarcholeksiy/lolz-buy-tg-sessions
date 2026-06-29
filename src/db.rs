use std::collections::HashSet;
use std::sync::Arc;
use sqlx::{PgConnection, PgPool};
use tokio::sync::Mutex;

#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "account_status", rename_all = "lowercase")]
pub enum AccountStatus {
    Active,
    Frozen,
    Unauthorized,
    DuplicatedKey,
}

#[derive(Debug)]
pub struct Account {
    pub app_id: i32,
    pub dc_id: i32,
    pub phone: Option<String>,
    pub session: String,
    pub status: AccountStatus,
    pub tg_id: i64,
    pub device_model: Option<String>,
    pub system_version: Option<String>,
    pub app_version: Option<String>,
}

pub async fn get_all_account_ids(pg_pool: &PgPool) -> anyhow::Result<Arc<Mutex<HashSet<i64>>>> {
    let ids = sqlx::query_scalar!(
        "SELECT tg_id FROM accounts"
    ).fetch_all(pg_pool).await?;

    let ids = ids.into_iter()
        .flatten()
        .collect::<HashSet<i64>>();

    Ok(Arc::new(Mutex::new(ids)))
}

pub async fn save_account(executor: &mut PgConnection, account: Account) -> Result<i32, sqlx::Error>{
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO accounts (app_id, dc_id, phone, session, status, tg_id, device_model, system_version, app_version)
        VALUES ($1, $2, $3, $4, $5::account_status, $6, $7, $8, $9)
        RETURNING id
        "#,
        account.app_id,
        account.dc_id,
        account.phone,
        account.session,
        account.status as _,
        account.tg_id,
        account.device_model,
        account.system_version,
        account.app_version
    ).fetch_one(executor).await?;
    Ok(id)
}

#[derive(Debug, Clone)]
pub struct TelegramApp {
    pub id: i32,
    pub api_id: i32,
    pub api_hash: String,
}

pub async fn get_telegram_app_by_id(pg_pool: &PgPool, id: i32) -> anyhow::Result<TelegramApp> {
    let telegram_app = sqlx::query_as!(
        TelegramApp,
        "SELECT id, api_id, api_hash FROM telegram_apps WHERE id = $1",
        id
    ).fetch_one(pg_pool).await?;
    Ok(telegram_app)
}

pub async fn get_or_create_seller_id(
    executor: &mut sqlx::PgConnection,
    market_user_id: i64,
    username: &str
) -> anyhow::Result<i32> {
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO sellers (market_user_id, username)
        VALUES ($1, $2)
        ON CONFLICT (market_user_id)
        DO UPDATE SET username = EXCLUDED.username
        RETURNING id
        "#,
        market_user_id,
        username
    ).fetch_one(executor).await?;
    Ok(id)
}

pub async fn save_purchase(
    pg_pool: &mut PgConnection,
    account_id: i32,
    seller_id: i32,
    price: i32
) -> anyhow::Result<()> {
    sqlx::query!(
        "INSERT INTO account_purchases (account_id, seller_id, price) VALUES ($1, $2, $3)",
        account_id, seller_id, price
    ).execute(pg_pool).await?;
    Ok(())
}