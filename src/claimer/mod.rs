use std::time::Duration;
use anyhow::bail;
use ferogram::InvocationError;
use sqlx::PgPool;
use tracing::{error, info, warn};
use crate::db::TelegramApp;
use crate::models::Order;

mod botfather;
mod codec;
mod legalizer;
mod repo;
mod tl_utils;

pub async fn process_order_to_session(pool: &PgPool, order: &Order, app: &TelegramApp) -> anyhow::Result<()> {
    let (session_str, dc_id) = codec::build_v2_session(
        &order.login_data.raw,
        &order.telegram_dc_id,
        order.telegram_id
    )?;

    const MAX_RETRIES: usize = 3;
    let mut attempts = 0;

    let client = loop {
        attempts += 1;
        match tl_utils::connect_auth_client(&session_str, app).await {
            Ok(c) => break c.clone(),
            Err(InvocationError::Rpc(rpc)) => {
                if rpc.code == 401 || rpc.code == 406 || rpc.name == "FROZEN_METHOD_INVALID" {
                    error!("Аккаунт {} мёртв при покупке: {}", order.telegram_phone, rpc.name);
                    repo::save_account(pool, order, app.id, dc_id as i32, &session_str, Some(rpc.code)).await?;
                    bail!("Аккаунт деактивирован Телеграмом: {}", rpc.code);
                }
            }
            Err(err) => {
                warn!("Сетевой сбой транспорта (попытка {}/{}): {err:#?}", attempts, MAX_RETRIES);
            }
        }

        if attempts >= MAX_RETRIES {
            error!("Таймаут для аккаунта: {}", order.telegram_phone);
            repo::save_account(pool, order, app.id, dc_id as i32, &session_str, Some(401)).await?;
            bail!("Не удалось подключить клиент после {} попыток из-за сетевых проблем", MAX_RETRIES);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };

    match tl_utils::resolve_botfather_peer(&client).await {
        Ok(peer_bot_father) => {
            match botfather::purge_mybots(&client, peer_bot_father).await {
                Ok(_) => info!("Process bot father complete"),
                Err(err) => warn!("Err process bot father: {err:?}")
            }
        }
        Err(err) => warn!("Err generate input peer for bot father: {err:?}")
    };

    let leg_client = match legalizer::spawn_legalized_client(&client, app, &order.telegram_phone).await {
        Ok(ct) => ct,
        Err(err) => {
            error!("Ошибка во время легализации аккаунта {}: {err:?}", order.telegram_phone);
            repo::save_account(pool, order, app.id, dc_id as i32, &session_str, Some(401)).await?;
            bail!("Аккаунт деактивирован Телеграмом: {err:?}");
        }
    };

    match leg_client.export_session_string().await {
        Ok(session_str) => {
            repo::save_account(pool, order, app.id, dc_id as i32, &session_str, None).await?;
            info!("Активный аккаунт {} сохранен!", order.telegram_phone);
        }
        Err(InvocationError::Rpc(rpc)) => {
            if rpc.code == 401 || rpc.code == 406 || rpc.name == "FROZEN_METHOD_INVALID" {
                error!("Аккаунт {} деактивирован при экспорте сессии: {}", order.telegram_phone, rpc.name);
                repo::save_account(pool, order, app.id, dc_id as i32, &session_str, Some(rpc.code)).await?;
                leg_client.disconnect();
                bail!("Аккаунт деактивирован Телеграмом: {}", rpc.code);
            }
        }
        Err(err) => {
            error!("Аккаунт {} деактивирован при экспорте сессии: {:?}", order.telegram_phone, err);
            repo::save_account(pool, order, app.id, dc_id as i32, &session_str, Some(401)).await?;
            leg_client.disconnect();
            bail!("Неизвестная ошибка: {err:?}");
        }
    }

    leg_client.disconnect();

    Ok(())
}