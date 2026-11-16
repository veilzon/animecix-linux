pub mod ffi;
pub mod loader;
pub mod player;
pub mod render;

/// Saniyeyi `MM:SS` veya `H:MM:SS` biçimine çevirir.
pub fn fmt_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--:--".into();
    }
    let s = seconds as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
