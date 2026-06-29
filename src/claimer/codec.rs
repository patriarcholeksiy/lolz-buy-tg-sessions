use anyhow::{bail, Context};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use std::net::Ipv4Addr;

pub fn build_v2_session(
    raw_login_data: &str,
    fallback_dc_str: &str,
    tg_id: i64,
) -> anyhow::Result<(String, u8)> {
    let (auth_key_hex, dc_id) = match raw_login_data.split_once(':') {
        Some((key, id)) => (key, id.parse::<u8>().unwrap_or(2)),
        None => (raw_login_data, fallback_dc_str.parse::<u8>().unwrap_or(2)),
    };

    let port: u16 = 443;
    let version_v2: u8 = 2;
    let ip_type_byte: u8 = 4;

    let ip = match dc_id {
        1 => Ipv4Addr::new(149, 154, 175, 53),
        2 => Ipv4Addr::new(149, 154, 167, 51),
        3 => Ipv4Addr::new(149, 154, 175, 100),
        4 => Ipv4Addr::new(149, 154, 167, 91),
        5 => Ipv4Addr::new(91, 108, 56, 165),
        _ => bail!("Неизвестный датацентр Telegram: {dc_id}"),
    };

    let key_bytes = hex::decode(auth_key_hex)
        .context("Ключ сессии не является валидной Hex-строкой")?;

    if key_bytes.len() != 256 {
        bail!("Неверная длина ключа: {} байт (ожидалось 256)", key_bytes.len());
    }

    let mut buffer = Vec::with_capacity(354);
    buffer.push(version_v2);                        // TL Version v2
    buffer.push(dc_id);                             // DC ID
    buffer.push(ip_type_byte);                      // IPv4 marker
    buffer.extend_from_slice(&ip.octets().to_vec());
    buffer.extend_from_slice(&port.to_be_bytes());
    buffer.extend_from_slice(&tg_id.to_be_bytes());
    buffer.extend_from_slice(&key_bytes);

    Ok((URL_SAFE_NO_PAD.encode(&buffer), dc_id))
}