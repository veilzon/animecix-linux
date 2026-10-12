use std::rc::Rc;
use std::sync::Arc;
use gtk::prelude::*;
use adw::prelude::*;
use crate::api::{Client, UserProfile};

pub fn show_login_dialog(
    parent: &impl IsA<gtk::Window>,
    client: Arc<Client>,
    on_success: impl Fn(UserProfile) + 'static,
) {
    let dialog = gtk::Window::builder()
        .title("AnimeciX Hesabı")
        .modal(true)
        .transient_for(parent)
        .default_width(460)
        .default_height(480)
        .resizable(false)
        .build();

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
    vbox.set_margin_top(20);
    vbox.set_margin_bottom(20);
    vbox.set_margin_start(20);
    vbox.set_margin_end(20);

    let title_lbl = gtk::Label::new(Some("AnimeciX Hesabına Giriş"));
    title_lbl.add_css_class("title-2");
    title_lbl.set_xalign(0.0);
    vbox.append(&title_lbl);

    let desc_lbl = gtk::Label::new(Some(
        "Hesabınıza giriş yaparak izleme geçmişinizi ve kaldığınız yeri senkronize edin."
    ));
    desc_lbl.add_css_class("dim-label");
    desc_lbl.set_wrap(true);
    desc_lbl.set_xalign(0.0);
    vbox.append(&desc_lbl);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);

    // 1. E-posta ile Giriş
    let email_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let email_group = adw::PreferencesGroup::new();
    let email_row = adw::EntryRow::new();
    email_row.set_title("E-posta veya Kullanıcı Adı");
    email_group.add(&email_row);

    let pass_row = adw::PasswordEntryRow::new();
    pass_row.set_title("Şifre");
    email_group.add(&pass_row);
    email_box.append(&email_group);
    stack.add_titled(&email_box, Some("email"), "Şifre ile Giriş");

    // 2. Çerez ile Bağla (Bypass Cloudflare)
    let cookie_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let cookie_info = gtk::Label::new(Some(
        "Tarayıcınızdan (F12 → Cookies veya Network sekmesi) kopyaladığınız connect.sid değerini veya doğrudan tüm Cookie metnini buraya yapıştırabilirsiniz. Gerekli çerezler otomatik ayrıştırılır."
    ));
    cookie_info.set_wrap(true);
    cookie_info.set_xalign(0.0);
    cookie_info.add_css_class("dim-label");
    cookie_box.append(&cookie_info);

    let cookie_group = adw::PreferencesGroup::new();
    let sid_row = adw::EntryRow::new();
    sid_row.set_title("connect.sid (veya Tüm Çerez Metni)");
    cookie_group.add(&sid_row);

    let xsrf_row = adw::EntryRow::new();
    xsrf_row.set_title("XSRF-TOKEN (isteğe bağlı)");
    cookie_group.add(&xsrf_row);

    let cf_row = adw::EntryRow::new();
    cf_row.set_title("cf_clearance (isteğe bağlı)");
    let cur_settings = client.load_settings();
    if !cur_settings.cf_clearance.is_empty() {
        cf_row.set_text(&cur_settings.cf_clearance);
    }
    cookie_group.add(&cf_row);
    cookie_box.append(&cookie_group);
    stack.add_titled(&cookie_box, Some("cookie"), "Çerez ile Bağla");

    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_halign(gtk::Align::Center);
    switcher.set_margin_bottom(8);
    vbox.append(&switcher);
    vbox.append(&stack);

    let error_lbl = gtk::Label::new(None);
    error_lbl.add_css_class("error");
    error_lbl.set_wrap(true);
    error_lbl.set_xalign(0.0);
    error_lbl.set_visible(false);
    vbox.append(&error_lbl);

    let spinner = gtk::Spinner::new();
    spinner.set_halign(gtk::Align::Center);
    spinner.set_visible(false);
    vbox.append(&spinner);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    btn_box.set_margin_top(12);

    let cancel_btn = gtk::Button::with_label("İptal");
    let dlg_c = dialog.clone();
    cancel_btn.connect_clicked(move |_| dlg_c.close());
    btn_box.append(&cancel_btn);

    let submit_btn = gtk::Button::with_label("Giriş Yap");
    submit_btn.add_css_class("suggested-action");
    submit_btn.add_css_class("pill");
    btn_box.append(&submit_btn);

    vbox.append(&btn_box);
    dialog.set_child(Some(&vbox));

    {
        let stack = stack.clone();
        let email_row = email_row.clone();
        let pass_row = pass_row.clone();
        let sid_row = sid_row.clone();
        let xsrf_row = xsrf_row.clone();
        let cf_row = cf_row.clone();
        let client = client.clone();
        let error_lbl = error_lbl.clone();
        let spinner = spinner.clone();
        let submit_btn = submit_btn.clone();
        let dialog = dialog.clone();
        let on_success = Rc::new(on_success);

        submit_btn.connect_clicked(move |btn| {
            error_lbl.set_visible(false);
            spinner.set_visible(true);
            spinner.start();
            btn.set_sensitive(false);

            let visible_child = stack.visible_child_name().unwrap_or_default();
            let client = client.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Result<UserProfile, String>>();

            if visible_child == "email" {
                let email = email_row.text().to_string();
                let pass = pass_row.text().to_string();
                std::thread::spawn(move || {
                    let res = client.login(&email, &pass);
                    let _ = tx.send(res);
                });
            } else {
                let sid = sid_row.text().to_string();
                let xsrf = xsrf_row.text().to_string();
                let cf = cf_row.text().to_string();
                std::thread::spawn(move || {
                    let xsrf_opt = if xsrf.is_empty() { None } else { Some(xsrf.as_str()) };
                    let cf_opt = if cf.is_empty() { None } else { Some(cf.as_str()) };
                    let res = client.login_with_cookie(&sid, xsrf_opt, cf_opt);
                    let _ = tx.send(res);
                });
            }

            let dialog_c = dialog.clone();
            let error_lbl_c = error_lbl.clone();
            let spinner_c = spinner.clone();
            let btn_c = btn.clone();
            let on_success_c = on_success.clone();

            glib::timeout_add_local(std::time::Duration::from_millis(20), move || match rx.try_recv() {
                Ok(result) => {
                    spinner_c.stop();
                    spinner_c.set_visible(false);
                    btn_c.set_sensitive(true);

                    match result {
                        Ok(user) => {
                            on_success_c(user);
                            dialog_c.close();
                        }
                        Err(err_msg) => {
                            error_lbl_c.set_text(&err_msg);
                            error_lbl_c.set_visible(true);
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    spinner_c.stop();
                    spinner_c.set_visible(false);
                    btn_c.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            });
        });
    }

    dialog.present();
}

#[cfg(test)]
mod tests {
    #[test]
    fn login_dialog_module_loads() {
        assert!(true);
    }
}
