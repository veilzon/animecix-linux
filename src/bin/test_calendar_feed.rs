// Gerçek takvim önbelleği üzerinde dedup davranışını doğrular.
// Kullanım: cargo run --bin test_calendar_feed [calendar.json]
#[allow(dead_code)]
#[path = "../calendar_feed.rs"]
mod calendar_feed;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        format!(
            "{}/.cache/animecix/api/calendar.json",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[TEST] takvim okunamadı: {path}: {e}");
            std::process::exit(2);
        }
    };
    let body = text.split_once('\n').map(|(_, b)| b).unwrap_or(&text);
    let mut v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[TEST] json hatası: {e}");
            std::process::exit(2);
        }
    };

    let count = |v: &serde_json::Value| -> usize {
        v["data"]
            .as_array()
            .map(|days| {
                days.iter()
                    .map(|d| d["episodes"].as_array().map(|a| a.len()).unwrap_or(0))
                    .sum()
            })
            .unwrap_or(0)
    };
    let before = count(&v);
    calendar_feed::dedup_calendar_json(&mut v);
    let after = count(&v);
    println!("[TEST] satır: {before} -> {after}");

    let mut found = 0;
    for day in v["data"].as_array().into_iter().flatten() {
        for e in day["episodes"].as_array().into_iter().flatten() {
            let id = e["title"]["id"].as_u64().unwrap_or(0);
            if matches!(id, 13445 | 13562 | 13452 | 13561) {
                found += 1;
                println!(
                    "[TEST] gün={} id={} S{}E{} rel={} est={} aliases={:?} alt={:?}",
                    day["date"].as_str().unwrap_or("?"),
                    id,
                    e["season_number"],
                    e["episode_number"],
                    e["release_date"].as_str().unwrap_or("?"),
                    e["estimated"],
                    e["aliases"].as_array().map(|a| a.len()).unwrap_or(0),
                    e["alt_title_ids"],
                );
            }
        }
    }
    if found != 2 {
        eprintln!("[TEST] HATA: Firefly/Hello birleşmiş satır sayısı {found}, beklenen 2");
        std::process::exit(1);
    }
    println!("[TEST] OK");
}
