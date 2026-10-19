//! Şarkı katmanı: bağlantı seçimi, `M` tuşu, kalıcı ASS altyazısı.

use crate::api::MusicData;

/// İzinli şarkı hostları (https zorunlu).
const SONG_HOSTS: [&str; 3] = ["open.spotify.com", "music.apple.com", "lis.tn"];

fn valid_song_url(u: &str) -> Option<String> {
    let s = u.trim();
    let rest = s.strip_prefix("https://")?;
    if s.contains(['\'', '"', '\n', '\r', ' ']) {
        return None;
    }
    let host = rest.split('/').next().unwrap_or("");
    if SONG_HOSTS.contains(&host) {
        Some(s.to_string())
    } else {
        None
    }
}

/// Şarkı bağlantısı: açılış öncelikli (Spotify > Apple > kısa link),
/// açılış yoksa kapanış. Geçersiz/eksikte None.
pub fn pick_song_url(op: Option<&MusicData>, ed: Option<&MusicData>) -> Option<String> {
    let one = |m: &MusicData| {
        [&m.spotify_url, &m.apple_music_url, &m.song_link]
            .into_iter()
            .flatten()
            .filter_map(|u| valid_song_url(u))
            .next()
    };
    op.and_then(one).or_else(|| ed.and_then(one))
}

/// `Shift+M` (`M`) tuşu satırı: şarkıyı varsayılan tarayıcıda açar (tüm distrolar:
/// xdg-open birincil, gio yedek) + şarkı satırının altında 3sn onay OSD'si.
pub fn music_keybind_line(url: &str) -> String {
    let safe = url.replace('\'', "%27");
    format!(
        "M run \"/bin/sh\" \"-c\" \"xdg-open '{safe}' || gio open '{safe}' || true\" ; \
         set osd-align-x right ; set osd-align-y top ; \
         set osd-margin-x 24 ; set osd-margin-y 85 ; \
         show-text \"🌐 tarayıcıda açıldı\" 3000\n"
    )
}

fn ass_time(sec: f64) -> String {
    let s = sec.max(0.0);
    let h = (s / 3600.0) as u64;
    let m = ((s % 3600.0) / 60.0) as u64;
    format!("{h}:{m:02}:{:05.2}", s % 60.0)
}

fn ass_text(s: &str) -> String {
    s.replace(['{', '}'], "").replace(['\n', '\r'], " ")
}

/// Kalıcı sağ-üst şarkı katmanı (ASS). Girdi: (başlangıç, bitiş, satır).
/// Şarkı yoksa boş döner (sub-file yazılmaz).
pub fn music_ass(
    op: Option<(f64, f64, &str)>,
    ed: Option<(f64, f64, &str)>,
    font: &str,
    show_hint: bool,
) -> String {
    let mut ev = String::new();
    let mut one = |tag: &str, from: f64, to: f64, line: &str| {
        // music_line() zaten "🎵 Açılış:" ön-eklidir; çiftlemeyi önle.
        let body = ass_text(line);
        let mut text = if body.starts_with("🎵") {
            format!("{{\\an9}}{}", body)
        } else {
            format!("{{\\an9}}🎵 {tag}: {}", body)
        };
        if show_hint {
            text.push_str(" (Shift+M: tarayıcıda aç)");
        }
        ev.push_str(&format!(
            "Dialogue: 0,{},{},Song,,0,0,0,,{}\n",
            ass_time(from),
            ass_time(to),
            text
        ));
    };
    if let Some((f, t, line)) = op {
        one("Açılış", f, t, line);
    }
    if let Some((f, t, line)) = ed {
        one("Kapanış", f, t, line);
    }
    if ev.is_empty() {
        return String::new();
    }
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\nScaledBorderAndShadow: yes\n\
         \n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Song,{font},34,&H00FFFFFF,&H90000000,&H90000000,0,0,1,2,0,9,24,24,24,1\n\
         \n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n{ev}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn music(spotify: Option<&str>, apple: Option<&str>, link: Option<&str>) -> MusicData {
        MusicData {
            title: "T".into(),
            artist: "A".into(),
            spotify_url: spotify.map(str::to_string),
            apple_music_url: apple.map(str::to_string),
            song_link: link.map(str::to_string),
        }
    }

    #[test]
    fn pick_song_url_priority_and_fallback() {
        let op = music(
            Some("https://open.spotify.com/track/x"),
            Some("https://music.apple.com/a"),
            Some("https://lis.tn/y"),
        );
        let ed = music(Some("https://open.spotify.com/track/e"), None, None);
        assert_eq!(
            pick_song_url(Some(&op), Some(&ed)).as_deref(),
            Some("https://open.spotify.com/track/x"),
            "spotify önce"
        );
        let op2 = music(
            None,
            Some("https://music.apple.com/a"),
            Some("https://lis.tn/y"),
        );
        assert_eq!(
            pick_song_url(Some(&op2), None).as_deref(),
            Some("https://music.apple.com/a"),
            "apple ikinci"
        );
        let op3 = music(None, None, Some("https://lis.tn/y"));
        assert_eq!(
            pick_song_url(Some(&op3), None).as_deref(),
            Some("https://lis.tn/y"),
            "kısa link son"
        );
        assert_eq!(
            pick_song_url(None, Some(&ed)).as_deref(),
            Some("https://open.spotify.com/track/e"),
            "açılış yoksa kapanış"
        );
        assert!(pick_song_url(None, None).is_none());
    }

    #[test]
    fn pick_song_url_rejects_bad() {
        for bad in [
            "http://open.spotify.com/track/x",
            "https://evil.com/track/x",
            "https://open.spotify.com/track/x y",
            "https://open.spotify.com/track/x'y",
            "",
        ] {
            let m = music(Some(bad), None, None);
            assert!(pick_song_url(Some(&m), None).is_none(), "elenmeli: {bad}");
        }
    }

    #[test]
    fn music_keybind_line_shape() {
        let l = music_keybind_line("https://open.spotify.com/track/x");
        assert!(l.starts_with("M run \"/bin/sh\" \"-c\""), "satır: {l}");
        assert!(
            l.contains("xdg-open 'https://open.spotify.com/track/x'"),
            "satır: {l}"
        );
        assert!(l.contains("gio open"), "yedek: {l}");
        assert!(l.contains("set osd-align-x right"), "sağ-üst: {l}");
        assert!(l.contains("set osd-align-y top"), "sağ-üst: {l}");
        assert!(l.contains("show-text"), "onay: {l}");
        assert!(l.contains("tarayıcıda açıldı"), "onay metni: {l}");
        assert!(l.contains("3000"), "süre: {l}");
    }

    #[test]
    fn ass_time_shapes() {
        assert_eq!(ass_time(0.0), "0:00:00.00");
        assert_eq!(ass_time(43.0), "0:00:43.00");
        assert_eq!(ass_time(133.0), "0:02:13.00");
        assert_eq!(ass_time(1352.0), "0:22:32.00");
    }

    #[test]
    fn music_ass_no_double_prefix() {
        let a = music_ass(
            Some((43.0, 133.0, "🎵 Açılış: Your Gaze — Tatsuya Kitani ▶")),
            None,
            "Montserrat SemiBold",
            true,
        );
        assert_eq!(a.matches("Açılış:").count(), 1, "ön-ek teklenmeli: {a}");
        assert!(a.contains("(Shift+M: tarayıcıda aç)"), "ipucu");
    }

    #[test]
    fn music_ass_shapes() {
        let a = music_ass(
            Some((43.0, 133.0, "T — A ▶")),
            None,
            "Montserrat SemiBold",
            true,
        );
        assert!(a.contains("{\\an9}"), "sağ üst");
        assert!(a.contains("0:00:43.00,0:02:13.00"), "zaman: {a}");
        assert!(
            a.contains("Fontname") && a.contains("Montserrat SemiBold"),
            "font"
        );
        assert!(a.contains("(Shift+M: tarayıcıda aç)"), "ipucu");
        let b = music_ass(Some((43.0, 133.0, "T {bozuk} — A")), None, "F", false);
        assert!(!b.contains("{bozuk}"), "başlık süslüsü temizlenmeli: {b}");
        assert!(!b.contains("tarayıcıda aç)"), "ipucu kapalı");
        assert!(music_ass(None, None, "F", true).is_empty(), "şarkısız boş");
    }
}
