use std::sync::Arc;
use std::time::Duration;
use anyhow::{bail, Context};
use ferogram::{tl, Client, InvocationError, NoRetries, OverflowStrategy};
use ferogram::tl::enums::contacts::ResolvedPeer;
use ferogram::tl::enums::{InputPeer, User};
use ferogram::tl::functions::contacts::{ResolvePhone, ResolveUsername};
use ferogram::tl::types::{InputPeerNotifySettings, InputPeerUser};
use crate::db::TelegramApp;

const BOT_FATHER_ID: i64 = 93372553;
const SERVICE_PHONE: &str = "42777";

pub async fn connect_auth_client(string_session: &String, telegram_app: &TelegramApp) -> Result<Client, InvocationError> {
    let config = Client::builder()
        .api_id(telegram_app.api_id)
        .api_hash(telegram_app.api_hash.clone())
        .update_queue_capacity(10)
        .session_string(string_session)
        .update_overflow_strategy(OverflowStrategy::DropOldest)
        .retry_policy(Arc::new(NoRetries))
        .system_lang_code("en")
        .device_model("Android")
        .app_version("3.4.5")
        .system_version("16")
        .build().unwrap();

    let (client, _) = Client::connect(config).await?;
    Ok(client)
}

pub async fn resolve_botfather_peer(client: &Client) -> anyhow::Result<InputPeer> {
    let ResolvedPeer::ResolvedPeer(peer) = client.invoke(&ResolveUsername {
        username: "BotFather".to_string(),
        referer: None,
    }).await?;
    let User::User(user) = peer.users.first().context("doesn't have user buy username")? else {
        bail!("Peer user have another enum type")
    };
    if user.id != BOT_FATHER_ID {
        bail!("Peer user id is not equal to the declared id")
    }
    Ok(InputPeer::User(InputPeerUser {
        user_id: user.id,
        access_hash: user.access_hash.context("access_hash is None")?,
    }))
}

pub async fn get_last_message(client: &Client, peer: InputPeer) -> anyhow::Result<tl::types::Message> {
    tokio::time::sleep(Duration::from_millis(500)).await;
    let history = client.invoke(&tl::functions::messages::GetHistory {
        peer,
        offset_id: 0,
        offset_date: 0,
        add_offset: 0,
        limit: 1,
        max_id: 0,
        min_id: 0,
        hash: 0,
    }).await?;

    // info!("{history:#?}");

    let tl::enums::messages::Messages::Slice(history) = history else { bail!("Can't parse last message") };
    let tl::enums::Message::Message(message) = history
        .messages
        .into_iter()
        .next()
        .context("Can't find first message")?
    else { bail!("Messages != type tl::types::Message") };
    Ok(message)
}

pub async fn resolve_service_chat_peer(client: &Client) -> anyhow::Result<InputPeer> {
    let ResolvedPeer::ResolvedPeer(peer) = client.invoke(&ResolvePhone {
        phone: SERVICE_PHONE.to_string(),
    }).await?;

    let User::User(user) = peer.users.first().context("doesn't have user buy username")? else {
        bail!("Peer user have another enum type")
    };

    let peer = InputPeer::User(InputPeerUser {
        user_id: user.id,
        access_hash: user.access_hash.context("access_hash is None")?,
    });
    Ok(peer)
}

pub async fn mute_chat(client: &Client, peer: InputPeer) -> anyhow::Result<()> {
    let set_settings = tl::enums::InputPeerNotifySettings::InputPeerNotifySettings(InputPeerNotifySettings {
        show_previews: None,
        silent: None,
        mute_until: Some(i32::MAX),
        sound: None,
        stories_muted: None,
        stories_hide_sender: None,
        stories_sound: None,
    });

    client.update_notify_settings(peer.clone(), set_settings).await?;

    Ok(())
}