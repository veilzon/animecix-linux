//! Kısayol yardım penceresi: uygulamanın gerçekten ele aldığı tuşlar.
//! (Lowell137/animecix-linux, MIT lisanslı uyarlama; içerik bizim tuşlara göre.)

use gtk::prelude::*;

/// Ayarlardaki "Ctrl+S" biçimini hızlandırıcı söz dizimine çevirir.
fn accel(sc: &str) -> String {
    match sc.strip_prefix("Ctrl+") {
        Some(k) => format!("<ctrl>{}", k.to_lowercase()),
        // "/" düz metin olarak ayrışmıyor; tuş adı verilmeli.
        None if sc == "/" => "slash".into(),
        None => sc.to_string(),
    }
}

/// Ayar değerleri kullanıcı dosyasından gelir; XML'e olduğu gibi girmez.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn item(sc: &str, title: &str) -> String {
    format!(
        r##"<child><object class="GtkShortcutsShortcut">
            <property name="accelerator">{}</property>
            <property name="title">{}</property>
          </object></child>"##,
        esc(sc),
        esc(title)
    )
}

fn group(title: &str, items: Vec<String>) -> String {
    format!(
        r##"<child><object class="GtkShortcutsGroup">
            <property name="title">{}</property>
            {}
          </object></child>"##,
        esc(title),
        items.concat()
    )
}

/// Yardım penceresinin arayüz tanımı.
fn ui_xml(search: &str, quick_search: &str, quick_enabled: bool) -> String {
    let groups = vec![group(
        "Genel",
        vec![
            item(&accel(search), "Aramayı aç"),
            item("F11", "Tam ekran"),
            item("Escape", "Kapat / tam ekrandan çık"),
            item("F1", "Bu pencere"),
        ],
    )];
    let mut out = groups;
    if quick_enabled {
        out.push(group(
            "Bölüm Sayfası",
            vec![item(&accel(quick_search), "Bölüm listesinde ara")],
        ));
    }
    // Harici mpv varsayılanları (bizde gömülü oynatıcı yok).
    out.push(group(
        "Oynatıcı (mpv)",
        vec![
            item("space", "Oynat / duraklat"),
            item("n", "Sonraki bölüm"),
            item("p", "Önceki bölüm"),
            item("f", "Tam ekran"),
            item("m", "Sesi aç / kapat"),
        ],
    ));
    format!(
        r#"<interface>
          <object class="GtkShortcutsWindow" id="shortcuts">
            <child><object class="GtkShortcutsSection">
              <property name="section-name">shortcuts</property>
              <property name="max-height">12</property>
              {}
            </object></child>
          </object>
        </interface>"#,
        out.concat()
    )
}

/// Kısayol penceresini açar; ayarlardan gelen tuşlar listeye işlenir.
pub fn present(
    parent: &impl IsA<gtk::Window>,
    search_shortcut: &str,
    quick_search: &str,
    quick_enabled: bool,
) {
    let builder = gtk::Builder::new();
    if let Err(e) = builder.add_from_string(&ui_xml(search_shortcut, quick_search, quick_enabled)) {
        eprintln!("[KISAYOL] pencere kurulamadı: {e}");
        return;
    }
    let Some(win) = builder.object::<gtk::ShortcutsWindow>("shortcuts") else {
        eprintln!("[KISAYOL] GtkShortcutsWindow bulunamadı");
        return;
    };
    win.set_transient_for(Some(parent));
    win.set_modal(true);
    win.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accel_uses_gtk_syntax() {
        assert_eq!(accel("Ctrl+S"), "<ctrl>s");
        assert_eq!(accel("/"), "slash");
        assert_eq!(accel("F2"), "F2");
    }

    #[test]
    fn xml_includes_settings_rows() {
        let with = ui_xml("F2", "Ctrl+F", true);
        assert!(!with.contains("Komut paleti"), "palet kalktı");
        assert!(with.contains("Aramayı aç"));
        assert!(with.contains("Tam ekran"));
        assert!(with.contains("Bölüm listesinde ara"));

        let without = ui_xml("Ctrl+S", "/", false);
        assert!(
            !without.contains("Bölüm listesinde ara"),
            "arama satırı gizlenmeli"
        );
        assert!(without.contains(r#"<property name="accelerator">&lt;ctrl&gt;s</property>"#));
    }

    #[test]
    fn xml_escapes_setting_values() {
        let x = ui_xml("<b>&x", "/", true);
        assert!(!x.contains("<b>"), "ham etiket kaçınılmalı");
        assert!(x.contains("&lt;b&gt;&amp;x"));
    }
}
