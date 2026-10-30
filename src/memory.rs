//! Oynatma sonrası bellek toparlama.
//!
//! MPV süreci kapandığında ana uygulamanın LRU kapak dokuları ve geçici
//! JSON/skip önbellekleri bırakılır; glibc heap'i `malloc_trim` ile
//! işletim sistemine geri verilir. Ağır iş yoktur, ana thread'de kısa
//! bir gecikmeyle çalışır.

use crate::covers::CoverManager;
use std::sync::Arc;
use std::time::Duration;

/// Oynatma kapandıktan sonra kısa bir nefes bırakıp önbellekleri süpürür.
pub fn trim_after_playback(covers: &CoverManager, client: &Arc<crate::api::Client>) {
    let covers = covers.clone_ref();
    let client = client.clone();
    glib::timeout_add_local_once(Duration::from_millis(1200), move || {
        covers.trim_budget(8 * 1024 * 1024);
        client.trim_transient_caches();
        let _ = trim_allocator();
    });
}

/// glibc heap'ini OS'a iade eder. Desteklenmeyen platformda sessizce false.
pub fn trim_allocator() -> bool {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        return libc::malloc_trim(0) != 0;
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(test)]
mod tests {
    use super::trim_allocator;

    #[test]
    fn trim_allocator_can_run_twice() {
        let _ = trim_allocator();
        let _ = trim_allocator();
    }
}
