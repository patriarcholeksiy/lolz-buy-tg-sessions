use crate::db::{get_or_create_seller_id, save_account as save_acc_db, save_purchase, Account, AccountStatus};
use crate::models::Order;
use anyhow::Context;
use sqlx::PgPool;

pub async fn save_account(
    pool: &PgPool,
    order: &Order,
    app_id: i32,
    dc_id: i32,
    session_str: &str,
    err_code: Option<i32>,
) -> anyhow::Result<i32> {
    let mut tx = pool.begin().await.context("Не удалось начать TX деактивации")?;

    let status = match err_code {
        None => AccountStatus::Active,
        Some(401) => AccountStatus::Unauthorized,
        Some(406) => AccountStatus::DuplicatedKey,
        _ => AccountStatus::Frozen,
    };

    let account = Account {
        app_id,
        dc_id,
        phone: if order.telegram_phone.is_empty() { None } else { Some(order.telegram_phone.clone()) },
        session: session_str.to_string(),
        status,
        tg_id: order.telegram_id,
    };

    let acc_id = save_acc_db(&mut *tx, account).await?;
    let seller_id = get_or_create_seller_id(&mut *tx, order.seller.user_id, &order.seller.username).await?;
    save_purchase(&mut *tx, acc_id, seller_id, order.price).await?;

    tx.commit().await?;
    Ok(acc_id)
}