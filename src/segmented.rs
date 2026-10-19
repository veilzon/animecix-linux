//! Parçalı (segmentli) indirme: aria-tarzı 6 eşzamanlı bağlantı.
//! Yeni harici bağımlılık yok; reqwest (blocking) + std::thread::scope.
//! Sunucu aralığı yoksayarsa/gizlerse tek-bağlantıya düşülür (çağıran karar verir).

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

use crate::download::{CHUNK, UA};

/// Altındaki dosyalar tek bağlantıyla iner (thread maliyeti değmez).
pub const MIN_SEGMENTED_BYTES: u64 = 8 << 20;

/// Toplam boyutu [başlangıç, bitiş] kapalı aralıklarına böler (son parça kalanı alır).
pub fn split_ranges(total: u64, n: usize) -> Vec<(u64, u64)> {
    if total == 0 {
        return Vec::new();
    }
    // Bayt sayısından çok parça olmaz (base==0 taşmasını önler).
    let n = n.max(1).min(total as usize) as u64;
    let base = total / n;
    let mut out = Vec::new();
    let mut start = 0u64;
    for i in 0..n {
        if start >= total {
            break;
        }
        let end = if i + 1 == n {
            total - 1
        } else {
            (start + base - 1).min(total - 1)
        };
        out.push((start, end));
        start = end + 1;
    }
    out
}

/// `Content-Range: bytes 0-0/12345` başlığından toplamı çözer.
pub fn parse_content_range_total(v: &str) -> Option<u64> {
    let (left, total) = v.split_once('/')?;
    if !left.starts_with("bytes ") || !left.contains('-') || left.contains('*') {
        return None;
    }
    total.trim().parse::<u64>().ok()
}

/// Sunucu aralıklı indirmeyi destekliyorsa toplam baytı döner.
/// Kural: HEAD uzunluğu + `bytes=0-0` isteğine 206 + Content-Range. Aksi halde None.
pub fn probe_len(
    client: &reqwest::blocking::Client,
    url: &str,
    referer: Option<&str>,
) -> Option<u64> {
    let head_total = client
        .head(url)
        .header("User-Agent", UA)
        .send()
        .ok()
        .and_then(|r| r.content_length())
        .filter(|t| *t > 0);
    let mut req = client
        .get(url)
        .header("User-Agent", UA)
        .header("Range", "bytes=0-0");
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    let resp = req.send().ok()?;
    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return None;
    }
    let total = resp
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_range_total)
        .filter(|t| *t > 0)?;
    // HEAD ile tutarlılık: çelişirse aralıklı yoldan vazgeç.
    if let Some(h) = head_total {
        if h != total {
            return None;
        }
    }
    // Gövdedeki 1 baytı tüket (bağlantı temizliği).
    let _ = resp.bytes();
    Some(total)
}

/// 6 segmenti eşzamanlı indirip `.part` dosyasına yazar.
/// `progress(toplam_yazılan, toplam)` raporlanır. İptal bayrağı her tur yoklanır.
/// Hata/iptal durumunda kısmi `.part` yerinde kalır (çağıran siler ya da tekliye düşer).
pub fn download_segmented(
    client: &reqwest::blocking::Client,
    url: &str,
    referer: Option<&str>,
    part: &Path,
    total: u64,
    n: usize,
    progress: &(dyn Fn(u64, u64) + Sync),
    cancel: &AtomicBool,
) -> Result<(), String> {
    // Hedefi önceden tahsis et (delikli yazma güvenli).
    {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(part)
            .map_err(|e| format!("dosya: {e}"))?;
        f.set_len(total).map_err(|e| format!("tahsis: {e}"))?;
    }
    let ranges = split_ranges(total, n);
    if ranges.is_empty() {
        return Err("boş aralık".to_string());
    }
    let done_sum = Arc::new(AtomicU64::new(0));
    let first_err: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let failed = Arc::new(AtomicBool::new(false));
    let first_err_c = first_err.clone();
    let failed_c = failed.clone();
    let record_fail = Arc::new(move |msg: String| {
        if let Ok(mut g) = first_err_c.lock() {
            if g.is_none() {
                *g = Some(msg);
            }
        }
        failed_c.store(true, Ordering::Relaxed);
    });

    std::thread::scope(|s| {
        for (start, end) in ranges {
            let seg_len = end - start + 1;
            let done_sum_c = done_sum.clone();
            let fail = record_fail.clone();
            let failed_i = failed.clone();
            s.spawn(move || {
                let mut req = client
                    .get(url)
                    .header("User-Agent", UA)
                    .header("Range", format!("bytes={start}-{end}"));
                if let Some(r) = referer {
                    req = req.header("Referer", r);
                }
                let mut resp = match req.send() {
                    Ok(r) => r,
                    Err(e) => {
                        fail(format!("bağlantı: {e}"));
                        return;
                    }
                };
                // 200 dönerse sunucu aralığı yoksaymıştır: segment OFSETİNE tam
                // gövde yazmak dosyayı bozar → kesin hata.
                if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                    fail(format!("aralık yoksayıldı (HTTP {})", resp.status()));
                    return;
                }
                // Content-Range başlangıcı segmentle eşleşmeli (ofset güvenliği).
                let cr_ok = resp
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.starts_with(&format!("bytes {start}-")))
                    .unwrap_or(false);
                if !cr_ok {
                    fail("content-range eşleşmedi".to_string());
                    return;
                }
                let mut file = match std::fs::OpenOptions::new().write(true).open(part) {
                    Ok(mut f) => {
                        if f.seek(SeekFrom::Start(start)).is_err() {
                            fail("seek hatası".to_string());
                            return;
                        }
                        f
                    }
                    Err(e) => {
                        fail(format!("dosya: {e}"));
                        return;
                    }
                };
                let mut buf = vec![0u8; CHUNK];
                let mut written: u64 = 0;
                let mut since_report: u64 = 0;
                loop {
                    if cancel.load(Ordering::Relaxed) || failed_i.load(Ordering::Relaxed) {
                        return;
                    }
                    match resp.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            written += n as u64;
                            if written > seg_len {
                                fail("segment taştı".to_string());
                                return;
                            }
                            if file.write_all(&buf[..n]).is_err() {
                                fail("yazma hatası (disk dolu?)".to_string());
                                return;
                            }
                            let sum = done_sum_c.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
                            since_report += n as u64;
                            if since_report >= CHUNK as u64 * 4 {
                                since_report = 0;
                                progress(sum, total);
                            }
                        }
                        Err(e) => {
                            fail(format!("ağ: {e}"));
                            return;
                        }
                    }
                }
                if written != seg_len {
                    fail(format!("eksik segment ({written}/{seg_len})"));
                }
            });
        }
    });

    if failed.load(Ordering::Relaxed) {
        if let Ok(g) = first_err.lock() {
            if let Some(e) = g.as_ref() {
                if !e.is_empty() {
                    return Err(e.clone());
                }
            }
        }
        return Err("segment hatası".to_string());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("iptal".to_string());
    }
    progress(total, total);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering as O};

    #[test]
    fn split_ranges_covers_exactly() {
        let r = split_ranges(100, 6);
        assert_eq!(r.len(), 6);
        assert_eq!(r[0].0, 0);
        assert_eq!(r[5].1, 99);
        for w in r.windows(2) {
            assert_eq!(w[0].1 + 1, w[1].0, "bitişik olmalı");
        }
        let total: u64 = r.iter().map(|(a, b)| b - a + 1).sum();
        assert_eq!(total, 100);
        assert_eq!(split_ranges(3, 6).len(), 3, "küçük dosya kırpılır");
        assert!(split_ranges(0, 6).is_empty());
    }

    #[test]
    fn parse_content_range_total_shapes() {
        assert_eq!(parse_content_range_total("bytes 0-0/12345"), Some(12345));
        assert_eq!(parse_content_range_total("bytes 10-20/99"), Some(99));
        assert_eq!(parse_content_range_total("bytes */99"), None);
        assert_eq!(parse_content_range_total("bozuk"), None);
    }

    /// Dilim sunucu: HEAD uzunluğu + 206 dilim + başlık iddiaları.
    fn slice_server(
        body_len: usize,
        ignore_range: bool,
    ) -> (u16, Arc<AtomicUsize>, Arc<std::sync::Mutex<Vec<String>>>) {
        static BODY_SEED: AtomicUsize = AtomicUsize::new(0);
        BODY_SEED.fetch_add(1, O::Relaxed);
        let listener = TcpListener::bind("127.0.0.1:0").expect("dinle");
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let hits_r = hits.clone();
        let seen_r = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(64) {
                let Ok(mut s) = stream else { continue };
                hits_r.fetch_add(1, O::Relaxed);
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                if let Ok(mut g) = seen_r.lock() {
                    g.push(req.clone());
                }
                let is_head = req.starts_with("HEAD ");
                // Wire'da başlık adları küçük harfle gelir (reqwest).
                let range = req.lines().find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("range: bytes=")
                        .map(|v| v.trim().to_string())
                });
                if is_head {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n"
                    );
                    let _ = s.write_all(head.as_bytes());
                    continue;
                }
                let (status, start, end) = match (ignore_range, range) {
                    (false, Some(r)) => {
                        let mut it = r.trim_end_matches('\r').split('-');
                        let a: usize = it.next().unwrap_or("0").parse().unwrap_or(0);
                        let b: usize = it
                            .next()
                            .and_then(|x| x.parse().ok())
                            .unwrap_or(body_len - 1);
                        let b = b.min(body_len - 1);
                        ("206 Partial Content", a, b)
                    }
                    _ => ("200 OK", 0, body_len - 1),
                };
                let blen = end - start + 1;
                let head = if status.starts_with("206") {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {blen}\r\nContent-Range: bytes {start}-{end}/{body_len}\r\nConnection: close\r\n\r\n"
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {blen}\r\nConnection: close\r\n\r\n"
                    )
                };
                let _ = s.write_all(head.as_bytes());
                let body: Vec<u8> = (start..=end).map(|i| (i % 251) as u8).collect();
                let _ = s.write_all(&body);
            }
        });
        (port, hits, seen)
    }

    fn test_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap()
    }

    #[test]
    fn probe_accepts_range_and_rejects_plain() {
        let (port, _, _) = slice_server(50_000, false);
        let c = test_client();
        let url = format!("http://127.0.0.1:{port}/v.mp4");
        assert_eq!(probe_len(&c, &url, None), Some(50_000));
        let (port2, _, _) = slice_server(50_000, true);
        let url2 = format!("http://127.0.0.1:{port2}/v.mp4");
        assert_eq!(
            probe_len(&c, &url2, None),
            None,
            "aralıksız sunucu elenmeli"
        );
    }

    #[test]
    fn segmented_assembly_6x_is_byte_exact() {
        const LEN: usize = 300_000;
        let (port, hits, seen) = slice_server(LEN, false);
        let c = test_client();
        let url = format!("http://127.0.0.1:{port}/v.mp4");
        let dir = std::env::temp_dir().join("animecix-seg");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("v.mp4.part");
        let cancel = AtomicBool::new(false);
        let total = probe_len(&c, &url, Some("http://ref/x")).expect("yoklama");
        assert_eq!(total, LEN as u64);
        let r = download_segmented(
            &c,
            &url,
            Some("http://ref/x"),
            &part,
            total,
            6,
            &|_, _| {},
            &cancel,
        );
        assert!(r.is_ok(), "segmentli: {r:?}");
        let got = std::fs::read(&part).unwrap();
        assert_eq!(got.len(), LEN);
        for (i, b) in got.iter().enumerate() {
            assert_eq!(*b, (i % 251) as u8, "ofset {i} bozuk");
        }
        assert!(hits.load(O::Relaxed) >= 7, "yoklama+6 segment");
        // UA + Referer 6 istekte de gitmeli.
        let g = seen.lock().unwrap();
        let lows: Vec<String> = g.iter().map(|r| r.to_ascii_lowercase()).collect();
        let seg_reqs: Vec<&String> = lows
            .iter()
            .filter(|r| r.contains("range: bytes=") && !r.contains("bytes=0-0"))
            .collect();
        assert_eq!(seg_reqs.len(), 6, "6 dilim isteği");
        for r in seg_reqs {
            assert!(r.contains("referer: http://ref/x"), "referer: {r}");
            assert!(r.contains("user-agent: mozilla"), "UA: {r}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_joins_all_segments() {
        use std::time::Duration;
        const LEN: usize = 50_000_000;
        let listener = TcpListener::bind("127.0.0.1:0").expect("dinle");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(32) {
                let Ok(mut s) = stream else { continue };
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                if req.starts_with("HEAD ") {
                    let _ = s.write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {LEN}\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    );
                    continue;
                }
                let low = req.to_ascii_lowercase();
                let is_probe = low.contains("range: bytes=0-0");
                // İstenen başlangıcı aynen yankıla (ofset doğrulaması geçsin).
                let want: usize = low
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|v| v.split('-').next()?.parse().ok())
                    .unwrap_or(0);
                let (a, b) = if is_probe { (0, 0) } else { (want, LEN - 1) };
                let blen = b - a + 1;
                let _ = s.write_all(
                    format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {blen}\r\nContent-Range: bytes {a}-{b}/{LEN}\r\nConnection: close\r\n\r\n").as_bytes(),
                );
                if is_probe {
                    let _ = s.write_all(&[0xABu8; 1]);
                } else {
                    // Dilim isteği: gövdeyi tut (iptal beklenir).
                    std::thread::sleep(Duration::from_secs(3));
                }
            }
        });
        let c = test_client();
        let url = format!("http://127.0.0.1:{port}/v.mp4");
        let dir = std::env::temp_dir().join("animecix-seg-cancel");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("v.mp4.part");
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_c = cancel.clone();
        let total = probe_len(&c, &url, None).expect("yoklama");
        let part_c = part.clone();
        let h = std::thread::spawn(move || {
            download_segmented(&c, &url, None, &part_c, total, 6, &|_, _| {}, &cancel_c)
        });
        std::thread::sleep(Duration::from_millis(400));
        cancel.store(true, O::Relaxed);
        let r = h.join().expect("join");
        assert!(r.is_err(), "iptal Err olmalı: {r:?}");
        assert!(part.exists(), ".part durmalı (devam edilebilir)");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
