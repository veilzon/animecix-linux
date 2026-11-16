//! MEGA public dosyaları için yerel AES-CTR proxy.
//!
//! MEGA içeriği istemci tarafında AES-128-CTR ile şifrelenir. mpv şifreli
//! byte'ları oynatamadığı için `mega.nz/embed/...` linkini indirilebilir
//! URL'ye çevirip, şifreyi çözerek `127.0.0.1` üzerinden mpv'ye sunuyoruz.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aes::Aes128;
use base64::Engine;
use ctr::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};

type Aes128Ctr = ctr::Ctr128BE<Aes128>;

const MEGA_API: &str = "https://g.api.mega.co.nz/cs?id=0";

#[derive(Clone, Debug, PartialEq)]
pub struct MegaLink {
    pub handle: String,
    pub key: String,
}

/// `mega.nz/embed/HANDLE#KEY`, `mega.nz/file/HANDLE#KEY` ve eski
/// `mega.nz/#!HANDLE!KEY` biçimlerini destekler.
pub fn parse_link(url: &str) -> Option<MegaLink> {
    let url = url.trim();
    let (rest, key) = url.split_once('#')?;
    if key.trim().is_empty() || key.contains('/') {
        return None;
    }
    if rest.contains("mega.nz/embed/") || rest.contains("mega.co.nz/embed/") {
        let handle = rest
            .split("/embed/")
            .nth(1)?
            .split(['?', '/'])
            .next()?
            .trim();
        if !handle.is_empty() {
            return Some(MegaLink {
                handle: handle.to_string(),
                key: key.to_string(),
            });
        }
    }
    if rest.contains("mega.nz/file/") || rest.contains("mega.co.nz/file/") {
        let handle = rest
            .split("/file/")
            .nth(1)?
            .split(['?', '/'])
            .next()?
            .trim();
        if !handle.is_empty() {
            return Some(MegaLink {
                handle: handle.to_string(),
                key: key.to_string(),
            });
        }
    }
    if let Some(legacy) = url.split_once("#!") {
        let parts: Vec<&str> = legacy.1.splitn(2, '!').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(MegaLink {
                handle: parts[0].to_string(),
                key: parts[1].to_string(),
            });
        }
    }
    None
}

pub struct MegaProxy {
    pub url: String,
    stop: Arc<AtomicBool>,
    _thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MegaProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Mega API'den dosya URL'sini çözer, yerel proxy'yi başlatır.
pub fn start_proxy(link: MegaLink) -> Result<MegaProxy, String> {
    let key = decode_key(&link.key)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("mega istemci: {e}"))?;
    let payload = serde_json::json!([{"a": "g", "g": 1, "p": link.handle}]);
    let value: serde_json::Value = client
        .post(MEGA_API)
        .json(&payload)
        .send()
        .map_err(|e| format!("mega api: {e}"))?
        .error_for_status()
        .map_err(|e| format!("mega api: {e}"))?
        .json()
        .map_err(|e| format!("mega json: {e}"))?;
    let item = value
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| "mega cevabı boş".to_string())?;
    if let Some(code) = item.as_i64() {
        return Err(format!("mega hata kodu: {code}"));
    }
    let download_url = item["g"]
        .as_str()
        .ok_or_else(|| "mega indirme URL'si yok".to_string())?
        .to_string();
    let size = item["s"]
        .as_u64()
        .ok_or_else(|| "mega dosya boyutu yok".to_string())?;

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("proxy bind: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("proxy nonblocking: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("proxy adres: {e}"))?
        .port();
    let token = format!(
        "{:x}{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let path = format!("/{token}/stream");
    let path_thread = path.clone();
    let thread = std::thread::spawn(move || {
        serve(
            listener,
            stop_thread,
            download_url,
            key.0,
            key.1,
            size,
            path_thread,
        );
    });

    Ok(MegaProxy {
        url: format!("http://127.0.0.1:{port}{path}"),
        stop,
        _thread: Some(thread),
    })
}

fn decode_key(raw: &str) -> Result<([u8; 16], [u8; 16]), String> {
    let trimmed = raw.trim().trim_end_matches('=');
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(trimmed)
        .map_err(|e| format!("mega anahtar: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("mega anahtar boyu {} (32 bekleniyor)", bytes.len()));
    }
    let mut aes_key = [0u8; 16];
    for i in 0..16 {
        aes_key[i] = bytes[i] ^ bytes[i + 16];
    }
    let mut iv = [0u8; 16];
    iv[..8].copy_from_slice(&bytes[16..24]);
    Ok((aes_key, iv))
}

fn serve(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    download_url: String,
    key: [u8; 16],
    iv: [u8; 16],
    size: u64,
    path: String,
) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let stop_c = stop.clone();
                let url = download_url.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    let _ = handle_client(stream, &url, key, iv, size, &path, &stop_c);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(60));
            }
            Err(_) => break,
        }
    }
}

fn handle_client(
    mut stream: TcpStream,
    download_url: &str,
    key: [u8; 16],
    iv: [u8; 16],
    size: u64,
    path: &str,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buf[..n]).to_string();
    let Some(first) = request.lines().next() else {
        return Ok(());
    };
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let req_path = parts.next().unwrap_or("");
    if req_path != path {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return Ok(());
    }

    let range = parse_range(&request, size);
    if let Some((start, end)) = range {
        if start >= size || start > end {
            let _ = stream.write_all(
                b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return Ok(());
        }
    }
    let (start, end, partial) = match range {
        Some((s, e)) => (s, e.min(size.saturating_sub(1)), true),
        None => (0, size.saturating_sub(1), false),
    };
    let length = end.saturating_sub(start).saturating_add(1);

    let status = if partial {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let mut header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: video/mp4\r\nAccept-Ranges: bytes\r\nContent-Length: {length}\r\nConnection: close\r\n"
    );
    if partial {
        header.push_str(&format!("Content-Range: bytes {start}-{end}/{size}\r\n"));
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes())?;
    if method == "HEAD" {
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(std::io::Error::other)?;
    let mut req = client
        .get(download_url)
        .header("User-Agent", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");
    if partial && start > 0 {
        req = req.header("Range", format!("bytes={start}-{end}"));
    }
    let mut response = req.send().map_err(std::io::Error::other)?;
    if !response.status().is_success() {
        return Ok(());
    }

    let mut cipher = Aes128Ctr::new(&key.into(), &iv.into());
    let mut remaining = length;
    let ranged = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if !ranged && start > 0 {
        // Sunucu Range'i yok saydı: başlangıç byte'larını at.
        let mut skip = start;
        let mut tmp = [0u8; 64 * 1024];
        while skip > 0 {
            let want = (skip as usize).min(tmp.len());
            let n = response.read(&mut tmp[..want])?;
            if n == 0 {
                return Ok(());
            }
            skip -= n as u64;
        }
    }
    cipher.seek(start);
    let mut chunk = [0u8; 64 * 1024];
    while remaining > 0 && !stop.load(Ordering::Relaxed) {
        let want = (remaining as usize).min(chunk.len());
        let n = response.read(&mut chunk[..want])?;
        if n == 0 {
            break;
        }
        cipher.apply_keystream(&mut chunk[..n]);
        stream.write_all(&chunk[..n])?;
        remaining -= n as u64;
    }
    Ok(())
}

fn parse_range(request: &str, size: u64) -> Option<(u64, u64)> {
    let line = request
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))?;
    let spec = line.split_once(':')?.1.trim();
    let spec = spec.strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = if end.trim().is_empty() {
        size.saturating_sub(1)
    } else {
        end.trim().parse().ok()?
    };
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_embed_file_and_legacy_links() {
        let a = parse_link("https://mega.nz/embed/BJBw3ZTL#waN4POyXZ2YjrkA7").unwrap();
        assert_eq!(a.handle, "BJBw3ZTL");
        let b = parse_link("https://mega.nz/file/BJBw3ZTL#waN4POyXZ2YjrkA7").unwrap();
        assert_eq!(b.handle, "BJBw3ZTL");
        let c = parse_link("https://mega.nz/#!BJBw3ZTL!waN4POyXZ2YjrkA7").unwrap();
        assert_eq!(c.handle, "BJBw3ZTL");
        assert!(parse_link("https://example.com/x#y").is_none());
    }

    #[test]
    fn parses_ranges() {
        let req = "GET /x HTTP/1.1\r\nRange: bytes=100-200\r\n\r\n";
        assert_eq!(parse_range(req, 1000), Some((100, 200)));
        let req = "GET /x HTTP/1.1\r\nRange: bytes=100-\r\n\r\n";
        assert_eq!(parse_range(req, 1000), Some((100, 999)));
        assert_eq!(parse_range("GET /x HTTP/1.1\r\n\r\n", 1000), None);
    }
}
