//! mpv atlama-şeridi mantığı: süre birleştirme, input.conf üretimi,
//! tuş/OSD komutları ve plan durumu. Ham soket ilkelleri player.rs'te,
//! API ayrıştırma api.rs'te kalır; burası ikisini mpv'ye bağlar.

use crate::api::{SkipTimes, CachedSkip, SkipMeta};
use crate::music;

/// Tek bölümün atlama durumu: süreler + şarkılar + kaynak etiketi.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkipPlan {
    pub times: SkipTimes,
    pub music_op: Option<String>,
    pub music_ed: Option<String>,
    pub song_url: Option<String>,
    pub source: String,
}

impl SkipPlan {
    /// Resmi cevaptan plan kurar (filtreli süreler + şarkılar + bağlantı).
    pub fn from_official(sm: &SkipMeta, source: &str) -> SkipPlan {
        SkipPlan {
            times: sm.sanitized_times(),
            music_op: sm.music_line(),
            music_ed: sm.outro_music_line(),
            song_url: music::pick_song_url(sm.music.as_ref(), sm.outro_music.as_ref()),
            source: source.to_string(),
        }
    }

    /// Önbellekten plan kurar.
    pub fn from_cached(cs: &CachedSkip, source: &str) -> SkipPlan {
        SkipPlan {
            times: cs.times.clone(),
            music_op: cs.music_op.clone(),
            music_ed: cs.music_ed.clone(),
            song_url: cs.song_url.clone(),
            source: source.to_string(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.times.op_end.is_none() && self.times.ed_end.is_none()
    }

    /// Önbelleğe giren veri (ömür bağımsız).
    pub fn cached(&self) -> CachedSkip {
        CachedSkip {
            times: self.times.clone(),
            music_op: self.music_op.clone(),
            music_ed: self.music_ed.clone(),
            song_url: self.song_url.clone(),
        }
    }
}

/// Video açılmadan önce atlama planını çözer (resolve worker'ında çağrılır).
/// Sıra: sır → meta (aynalar, yedek best-video) → önbellek → most-sought (retry).
/// Başarısızlıkta None + stderr'e sebep (ağ-hatası vs boş-DB ayrımıyla).
pub fn fetch_plan_for_embeds(
    client: &crate::api::Client,
    title_id: u64,
    season: u64,
    episode: u64,
    fast_embeds: &[String],
    fallback_embeds: &[String],
    manual_secret: &str,
) -> Option<SkipPlan> {
    let (secret, src) = match client.resolve_skip_secret(manual_secret) {
        Some(p) => p,
        None => {
            eprintln!("[SKIP-RESMI] sır yok (kasa derlenmemiş, kutu boş)");
            return None;
        }
    };
    let mut meta = client
        .tau_meta_from_embeds(fast_embeds)
        .or_else(|| client.tau_meta_from_embeds(fallback_embeds));
    if meta.is_none() {
        match client.resolve_best_video_full(title_id, episode, season) {
            Ok((_, m)) => meta = m,
            Err(e) => eprintln!("[SKIP-RESMI] best-video: {e}"),
        }
    }
    let m = meta?;
    let slug = m.most_sought_slug();
    if let Some(cs) = client.skip_cache_get(&slug) {
        eprintln!("[SKIP-RESMI] (önbellek) slug={slug}");
        return Some(SkipPlan::from_cached(&cs, "kasa (önbellek)"));
    }
    match client.fetch_official_skip(&m, &secret) {
        Ok(sm) => {
            let plan = SkipPlan::from_official(&sm, src);
            if plan.is_empty() {
                eprintln!("[SKIP-RESMI] (kaynak={src}) slug={slug} resmi boş (DB'de yok)");
                return None;
            }
            eprintln!(
                "[SKIP-RESMI] (kaynak={src}) slug={slug} op={:?}-{:?} ed={:?}-{:?}",
                plan.times.op_start, plan.times.op_end,
                plan.times.ed_start, plan.times.ed_end
            );
            client.skip_cache_put(&slug, &plan.cached());
            Some(plan)
        }
        Err(e) => {
            eprintln!("[SKIP-RESMI] slug={slug} alınamadı: {e:?}");
            None
        }
    }
}

/// mpv input.conf içeriği. Boş sürelerde "bulunamadı" metni yazar
/// (ebedi placeholder yerine dürüst durum).
/// Atlama OSD'leri sol-alta sabitlenir (M tuşu sağ-üste taşımış olabilir).
pub fn input_conf(t: &SkipTimes, source: &str) -> String {
    let fmt_sec = |sec: f64| -> String {
        let s = sec as u64;
        format!("{:02}:{:02}", s / 60, s % 60)
    };
    let align = "set osd-align-x left; set osd-align-y bottom; set osd-margin-x 30; set osd-margin-y 30;";
    let skip_cmd = if let (Some(st), Some(et)) = (t.op_start, t.op_end) {
        format!("s seek {et:.1} absolute; {align} show-text \"⏩ İntro Atlandı ({source}: {} → {})\" 3000\n", fmt_sec(st), fmt_sec(et))
    } else {
        format!("s {align} show-text \"⚠️ İntro zamanı bulunamadı\" 2500\n")
    };
    let outro_cmd = if let (Some(st), Some(et)) = (t.ed_start, t.ed_end) {
        format!("e seek {et:.1} absolute; {align} show-text \"⏩ Outro Atlandı ({source}: {} → {})\" 3000\n", fmt_sec(st), fmt_sec(et))
    } else {
        format!("e {align} show-text \"⚠️ Outro zamanı bulunamadı\" 2500\n")
    };
    format!("{skip_cmd}{outro_cmd}S seek -30; {align} show-text \"⏪ 30s Geri\" 2000\nEnd ignore\n")
}

/// Sonraki bölüm kısayolları: '>', 'N', 'Ctrl+RIGHT'.
/// Basıldığında bayrak dosyasına dokunur (touch) ve mpv'yi kapatarak animecix'e sonraki bölümü açtırır.
pub fn next_keybind_lines(flag_path: &str) -> String {
    let align = "set osd-align-x left; set osd-align-y bottom; set osd-margin-x 30; set osd-margin-y 30;";
    format!(
        "> run \"/usr/bin/touch\" \"{flag_path}\" ; {align} show-text \"⏭ Sonraki Bölüm Açılıyor...\" 2500 ; quit\n\
         N run \"/usr/bin/touch\" \"{flag_path}\" ; {align} show-text \"⏭ Sonraki Bölüm Açılıyor...\" 2500 ; quit\n\
         Ctrl+RIGHT run \"/usr/bin/touch\" \"{flag_path}\" ; {align} show-text \"⏭ Sonraki Bölüm Açılıyor...\" 2500 ; quit\n"
    )
}

/// mpv IPC JSON'una gömülecek metni güvenli yapar (tırnak/tersbölü kırar).
pub fn mpv_safe_text(s: &str) -> String {
    s.replace('\\', "/").replace('"', "'")
}

/// Tek noktadan OSD komutu kurar (kaçış dahil).
pub fn show_text_cmd(text: &str, ms: u32) -> String {
    let safe = mpv_safe_text(text);
    format!("{{\"command\":[\"show-text\", \"{safe}\", {ms}]}}\n")
}

/// Atlama OSD dizisi: önce sol-alt hizayı ilan eder (M tuşu sağ-üste
/// taşımış olabilir), sonra metni gösterir. Kendini iyileştirir.
pub fn skip_osd_cmds(text: &str, ms: u32) -> Vec<String> {
    vec![
        "{\"command\":[\"set_property\", \"osd-align-x\", \"left\"]}\n".to_string(),
        "{\"command\":[\"set_property\", \"osd-align-y\", \"bottom\"]}\n".to_string(),
        "{\"command\":[\"set_property\", \"osd-margin-x\", 30]}\n".to_string(),
        "{\"command\":[\"set_property\", \"osd-margin-y\", 30]}\n".to_string(),
        show_text_cmd(text, ms),
    ]
}

/// İntro başlangıç bildirimi (şarkı varsa eklenir).
pub fn prompt_op(music: Option<&str>) -> String {
    let mut msg = "⏩ İntro Başladı ('s' ile atlayabilirsiniz)".to_string();
    if let Some(m) = music {
        msg.push_str(&format!(" • {m}"));
    }
    msg
}

/// Outro başlangıç bildirimi (şarkı varsa eklenir).
pub fn prompt_ed(music: Option<&str>) -> String {
    let mut msg = "🏁 Outro Başladı".to_string();
    if let Some(m) = music {
        msg.push_str(&format!(" • {m}"));
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::SkipTimes;

    #[test]
    fn plan_from_official_jjk_case() {
        let sm = crate::api::parse_skip_meta(&serde_json::json!({
            "intro": {"from": 63, "to": 147},
            "outro": {"from": 8, "to": 4},
        }));
        let p = SkipPlan::from_official(&sm, "kasa");
        assert_eq!((p.times.op_start, p.times.op_end), (Some(63.0), Some(147.0)));
        assert!(p.times.ed_start.is_none() && p.times.ed_end.is_none(), "çöp outro elenmeli");
        assert!(!p.is_empty());
        assert!(p.music_op.is_none() && p.music_ed.is_none());
        assert_eq!(SkipPlan::default().is_empty(), true);
    }

    #[test]
    fn skip_osd_cmds_pin_bottom_left() {
        let cmds = skip_osd_cmds("⏩ İntro", 4000);
        assert_eq!(cmds.len(), 5, "4 hiza + 1 metin");
        assert!(cmds[0].contains("osd-align-x") && cmds[0].contains("left"));
        assert!(cmds[1].contains("osd-align-y") && cmds[1].contains("bottom"));
        assert!(cmds[4].contains("show-text") && cmds[4].contains("4000"));
    }

    #[test]
    fn input_conf_shapes() {
        let t = SkipTimes { op_start: Some(43.0), op_end: Some(133.0), ed_start: None, ed_end: None };
        let c = input_conf(&t, "resmi kaynak");
        assert!(c.contains("s seek 133.0 absolute"), "tuş: {c}");
        assert!(c.contains("resmi kaynak: 00:43 → 02:13"), "kaynak+aralık: {c}");
        assert!(c.contains("Outro zamanı bulunamadı"), "eksik çift: {c}");
        let empty = input_conf(&SkipTimes::default(), "x");
        assert!(empty.contains("İntro zamanı bulunamadı") && empty.contains("Outro zamanı bulunamadı"), "boş conf dürüst olmalı: {empty}");
        assert!(empty.contains("S seek -30"), "geri tuşu korunmalı");
    }

    #[test]
    fn mpv_safe_text_neutralizes_json_breakers() {
        let out = mpv_safe_text("🎵 \"Quoted\" \\ Title — A");
        assert!(!out.contains('"'), "tırnak kalmamalı: {out}");
        assert!(!out.contains('\\'), "tersbölü kalmamalı: {out}");
        assert!(out.contains("Quoted") && out.contains("Title"), "içerik korunmalı: {out}");
    }

    #[test]
    fn show_text_and_prompts() {
        let cmd = show_text_cmd("a \"b\" c", 8000);
        assert!(cmd.contains("a 'b' c") && cmd.contains("8000"), "cmd: {cmd}");
        let v: serde_json::Value = serde_json::from_str(cmd.trim()).expect("IPC JSON geçerli olmalı");
        assert_eq!(v["command"][0], "show-text");
        assert!(prompt_op(None).contains("İntro Başladı"));
        assert!(prompt_op(Some("🎵 X — Y")).contains("🎵 X — Y"), "şarkı eklenmeli");
        assert!(prompt_ed(None).contains("Outro Başladı"));
        assert!(prompt_ed(Some("🎵 Z")).contains("🎵 Z"));
    }

    #[test]
    fn next_keybind_lines_includes_expected_keys() {
        let lines = next_keybind_lines("/tmp/flag.txt");
        assert!(lines.contains("> run \"/usr/bin/touch\" \"/tmp/flag.txt\""));
        assert!(lines.contains("N run \"/usr/bin/touch\" \"/tmp/flag.txt\""));
        assert!(lines.contains("Ctrl+RIGHT run \"/usr/bin/touch\" \"/tmp/flag.txt\""));
        assert!(lines.contains("quit"));
    }
}
