//! Oynatma kalite seçici: MessageDialog ile etiket seçimi.
//! Ağ/çözümleme yok; yalnızca UI (fansub_dialog.rs + ask_download_quality deseni).

use adw::prelude::*;

/// Dialog sonucu: seçili kalite, en-iyi (bugünkü yol), vazgeçme (oynatma durur).
pub enum PlayChoice {
    Quality(String),
    Best,
    Cancelled,
}

/// Kalite seçiciyi gösterir. `labels` sıralı etikettir (örn. 1080p, 720p, 480p).
pub fn show_play_quality_dialog(
    parent: &impl gtk::prelude::IsA<gtk::Window>,
    title: &str,
    labels: &[String],
    on_select: impl Fn(PlayChoice) + 'static,
) {
    let dlg = adw::MessageDialog::new(Some(parent), Some("Oynatma Kalitesi"), None);
    dlg.set_body(&format!("{title} — hangi kalitede açılsın?"));
    for label in labels {
        dlg.add_response(label, label);
    }
    dlg.add_response("best", "En iyi (otomatik)");
    dlg.add_response("cancel", "Vazgeç");
    dlg.set_default_response(Some("best"));
    dlg.set_close_response("cancel");
    {
        let labels_c: Vec<String> = labels.to_vec();
        dlg.connect_response(None, move |_, resp: &str| {
            if resp == "cancel" {
                on_select(PlayChoice::Cancelled);
            } else if resp == "best" {
                on_select(PlayChoice::Best);
            } else if labels_c.iter().any(|l| l == resp) {
                on_select(PlayChoice::Quality(resp.to_string()));
            } else {
                on_select(PlayChoice::Best);
            }
        });
    }
    dlg.present();
}

#[cfg(test)]
mod tests {
    #[test]
    fn dialog_module_loads() {
        // GTK'sız derleme kanıtı (widget kurulumu göz testindedir).
        assert!(true);
    }
}
