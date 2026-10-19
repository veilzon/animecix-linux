use crate::api::FansubInfo;
use gtk::prelude::*;
use std::rc::Rc;

pub fn show_fansub_dialog(
    parent: &impl IsA<gtk::Window>,
    title: &str,
    fansubs: Vec<FansubInfo>,
    on_select: impl Fn(FansubInfo) + 'static,
) {
    let dialog = gtk::Window::builder()
        .title(title)
        .modal(true)
        .transient_for(parent)
        .default_width(420)
        .default_height(360)
        .resizable(false)
        .build();

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let header = gtk::Label::new(Some("Çeviri Seç"));
    header.add_css_class("title-2");
    header.set_xalign(0.0);
    vbox.append(&header);

    let count_text = if fansubs.is_empty() {
        "Çeviri bulunamadı".to_string()
    } else {
        format!("{} çeviri mevcut", fansubs.len())
    };
    let subtitle = gtk::Label::new(Some(&count_text));
    subtitle.add_css_class("dim-label");
    subtitle.set_xalign(0.0);
    vbox.append(&subtitle);

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let on_select = Rc::new(on_select);

    for fs in &fansubs {
        let row = gtk::Button::new();
        row.add_css_class("card");
        row.set_halign(gtk::Align::Fill);

        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        hbox.set_margin_top(10);
        hbox.set_margin_bottom(10);
        hbox.set_margin_start(12);
        hbox.set_margin_end(12);

        let name_lbl = gtk::Label::new(Some(&fs.name));
        name_lbl.add_css_class("title-4");
        name_lbl.set_xalign(0.0);
        name_lbl.set_hexpand(true);
        name_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        hbox.append(&name_lbl);

        if fs.rating > 0.0 {
            let stars = format!("★ {:.1}", fs.rating);
            let pts_lbl = gtk::Label::new(Some(&stars));
            pts_lbl.add_css_class("accent");
            pts_lbl.set_xalign(0.5);
            hbox.append(&pts_lbl);
        }

        if !fs.approved_only {
            let warn_lbl = gtk::Label::new(Some("eski"));
            warn_lbl.add_css_class("dim-label");
            warn_lbl.set_xalign(0.5);
            hbox.append(&warn_lbl);
        }

        row.set_child(Some(&hbox));

        let fs_clone = fs.clone();
        let on_sel = on_select.clone();
        let dlg = dialog.clone();
        row.connect_clicked(move |_| {
            on_sel(fs_clone.clone());
            dlg.close();
        });

        list.append(&row);
    }

    scrolled.set_child(Some(&list));
    vbox.append(&scrolled);

    let close_btn = gtk::Button::with_label("İptal");
    close_btn.set_halign(gtk::Align::End);
    let dlg2 = dialog.clone();
    close_btn.connect_clicked(move |_| dlg2.close());
    vbox.append(&close_btn);

    dialog.set_child(Some(&vbox));
    dialog.present();
}
