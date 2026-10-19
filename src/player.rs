use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub fn query_mpv_prop(sock: &str, prop: &str) -> Option<f64> {
    let mut stream = UnixStream::connect(sock).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(400)))
        .ok();
    stream
        .set_write_timeout(Some(Duration::from_millis(400)))
        .ok();
    let cmd = format!("{{\"command\":[\"get_property\",\"{prop}\"]}}\n");
    stream.write_all(cmd.as_bytes()).ok()?;
    let mut buf = vec![0u8; 512];
    let n = stream.read(&mut buf).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&buf[..n]).ok()?;
    if v["error"].as_str() != Some("success") {
        return None;
    }
    match v["data"] {
        serde_json::Value::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
        _ => v["data"].as_f64(),
    }
}

pub fn query_mpv_position(sock: &str) -> Option<(f64, f64)> {
    let mut stream = UnixStream::connect(sock).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(400)))
        .ok();
    stream
        .set_write_timeout(Some(Duration::from_millis(400)))
        .ok();

    let cmd1 = r#"{"command":["get_property","time-pos"]}"#;
    let cmd2 = r#"{"command":["get_property","duration"]}"#;
    stream.write_all(cmd1.as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;
    stream.write_all(cmd2.as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;

    let mut buf = vec![0u8; 512];
    let n1 = stream.read(&mut buf).ok()?;
    let v1: serde_json::Value = serde_json::from_slice(&buf[..n1]).ok()?;
    let pos = if v1["error"].as_str() == Some("success") {
        v1["data"].as_f64().unwrap_or(0.0)
    } else {
        return None;
    };

    let n2 = stream.read(&mut buf).ok()?;
    let v2: serde_json::Value = serde_json::from_slice(&buf[..n2]).ok()?;
    let dur = if v2["error"].as_str() == Some("success") {
        v2["data"].as_f64().unwrap_or(0.0)
    } else {
        0.0
    };

    Some((pos, dur))
}

pub fn send_mpv_cmd(sock: &str, json_cmd: &str) -> bool {
    let Ok(mut stream) = UnixStream::connect(sock) else {
        return false;
    };
    stream
        .set_write_timeout(Some(Duration::from_millis(400)))
        .ok();
    stream.write_all(json_cmd.as_bytes()).is_ok()
}

/// Komutu birkaç kez dener; sonunda da olmazsa stderr'e yazar.
/// Soket yarışlarında sessiz kayıpları bitirir.
pub fn send_mpv_cmd_retry(sock: &str, json_cmd: &str, tries: u32) -> bool {
    for _ in 0..tries.max(1) {
        if send_mpv_cmd(sock, json_cmd) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(700));
    }
    eprintln!(
        "[MPV-IPC] komut iletilemedi ({} deneme): {}",
        tries.max(1),
        json_cmd.trim()
    );
    false
}

/// Manuel bölüm geçişi çıkış kodları (`input.conf`: `n quit 42`, `p quit 43`).
/// mpv `quit <kod>` ile kapanır; supervisor kodu okuyup komşu bölüme geçer.
pub const MPV_EXIT_NEXT: i32 = 42;
pub const MPV_EXIT_PREV: i32 = 43;

/// mpv çıkış kodunu bölüm yönüne çevirir (+1 sonraki, -1 önceki).
/// `None` = normal çıkış / kaynak hatası (geçiş yok).
pub fn mpv_exit_direction(code: Option<i32>) -> Option<i8> {
    match code {
        Some(c) if c == MPV_EXIT_NEXT => Some(1),
        Some(c) if c == MPV_EXIT_PREV => Some(-1),
        _ => None,
    }
}

/// Otomatik geçiş tetiklensin mi? (saf; tek-atımlık guard çağıranda).
/// Koşullar: ayar açık + henüz ateşlenmedi + EOF bayrağı + süreli içerikte
/// izlenme eşiği aşıldı (yanlış tetikleme: failover'da `dur<=0` elenir).
pub fn should_auto_next(
    pos: f64,
    dur: f64,
    eof_reached: bool,
    auto_enabled: bool,
    already_fired: bool,
) -> bool {
    auto_enabled
        && !already_fired
        && eof_reached
        && dur > 0.0
        && crate::api::Client::played_enough(pos, dur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_direction_maps_next_prev() {
        assert_eq!(mpv_exit_direction(Some(42)), Some(1));
        assert_eq!(mpv_exit_direction(Some(43)), Some(-1));
        assert_eq!(mpv_exit_direction(Some(0)), None);
        assert_eq!(mpv_exit_direction(None), None);
        assert_eq!(mpv_exit_direction(Some(1)), None);
    }

    #[test]
    fn auto_next_needs_all_conditions() {
        assert!(should_auto_next(95.0, 100.0, true, true, false));
        assert!(!should_auto_next(95.0, 100.0, true, false, false));
        assert!(!should_auto_next(95.0, 100.0, false, true, false));
        assert!(!should_auto_next(95.0, 100.0, true, true, true));
        assert!(!should_auto_next(45.0, 100.0, true, true, false));
        assert!(!should_auto_next(95.0, 0.0, true, true, false));
    }
}
