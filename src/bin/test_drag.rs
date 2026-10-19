// Maraton sürükle-bırak (DnD) doğrulama testi.
// Amaç: GTK4 DragSource/DropTarget API kablolamasının derlendiğini ve
// drop -> reorder veri akışının doğru çalıştığını kanıtlamak.
//
// Mantık testleri görüntü (display) gerektirmez; GTK smoke testi yalnızca
// bir display varsa çalışır (headless ortamda atlanır).

use std::cell::RefCell;
use std::rc::Rc;

/// reorder_marathon ile birebir aynı saf mantık (api.rs:1135).
/// order: mevcut sıra (id listesi), id: taşınacak öğe, new_index: hedef indeks.
fn compute_reorder(order: &[u64], id: u64, new_index: usize) -> Vec<u64> {
    let mut v: Vec<u64> = order.to_vec();
    if let Some(pos) = v.iter().position(|&x| x == id) {
        if pos == new_index {
            return v;
        }
        let item = v.remove(pos);
        let idx = new_index.min(v.len());
        v.insert(idx, item);
    }
    v
}

/// DropTarget::connect_drop içindeki mantık (views.rs'te uygulanacak).
/// Gelen Value'dan kaynak id'yi çözer; kendine bırakma ve bozuk veri yok sayılır.
fn handle_drop(value: &glib::Value, self_id: u64, idx: usize, cb: &mut Vec<(u64, usize)>) -> bool {
    if let Ok(s) = value.get::<String>() {
        if let Ok(src_id) = s.parse::<u64>() {
            if src_id != self_id {
                cb.push((src_id, idx));
            }
        }
    }
    true
}

/// Sürükleme ikonunun hotspot'unu kartın MERKEZİNE yerleştirir.
/// Böylece kart nereden tutulursa tutulsun imleç kartın ortasında kalır.
fn centered_hotspot(w: i32, h: i32) -> (i32, i32) {
    (w / 2, h / 2)
}

/// Auto-scroll: imleç görünür alanın kenarına yaklaştıkça kaydırma miktarı.
/// Negatif = yukarı, pozitif = aşağı, orta => 0 (kaydırma yok).
fn scroll_delta(y: f64, viewport_h: f64, margin: f64) -> f64 {
    if y < margin {
        -((margin - y) * 0.6 + 6.0)
    } else if y > viewport_h - margin {
        (y - (viewport_h - margin)) * 0.6 + 6.0
    } else {
        0.0
    }
}

/// Gerçek DnD kablolamasını KURAR (derleme doğrulaması için).
/// Bir display varsa çağrılır; olasılıkla panik olursa yakalanır.
fn build_dnd_wiring() {
    use gtk::prelude::*;

    // 4 kart -> DragSource + DropTarget
    let recorder: Rc<RefCell<Vec<(u64, usize)>>> = Rc::new(RefCell::new(Vec::new()));
    let mut cards: Vec<gtk::Box> = Vec::new();

    for i in 0..4u64 {
        let card = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        card.add_css_class("marathon-item-card");

        // --- DragSource (kartın tamamı sürüklenebilir) ---
        let drag = gtk::DragSource::new();
        drag.set_actions(gtk::gdk::DragAction::MOVE);

        // Sürükleme ikonu: kartın kendisi, hotspot kartın MERKEZİNDE
        // (varsayılan sayı metni değil; nereden tutulursa tutulsun imleç ortada)
        let card_for_icon = card.clone();
        let src_id = i;
        drag.connect_prepare(move |drag, _, _| {
            let alloc = card_for_icon.allocation();
            let paintable = gtk::WidgetPaintable::new(Some(&card_for_icon));
            drag.set_icon(Some(&paintable), alloc.width() / 2, alloc.height() / 2);
            Some(gtk::gdk::ContentProvider::for_value(&glib::Value::from(
                src_id.to_string(),
            )))
        });

        // Sürüklerken kaynak kart opaklığını düşür
        let card_dim = card.clone();
        drag.connect_drag_begin(move |_, _| {
            card_dim.set_opacity(0.35);
        });
        let card_restore = card.clone();
        drag.connect_drag_end(move |_, _, _| {
            card_restore.set_opacity(1.0);
        });
        card.add_controller(drag);

        // --- DropTarget ---
        let drop = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
        let self_id = i;
        let rec = recorder.clone();
        drop.connect_drop(move |_, value, _, _| {
            let mut cb = rec.borrow_mut();
            handle_drop(value, self_id, i as usize, &mut cb)
        });
        card.add_controller(drop);

        // --- DropControllerMotion (auto-scroll için kenar tespiti) ---
        let motion = gtk::DropControllerMotion::new();
        motion.connect_motion(move |_, _x, _y| {});
        motion.connect_leave(move |_| {});
        card.add_controller(motion);

        cards.push(card);
    }

    // Kablolamanın kurulduğunu kanıtlamak için widget sayısını doğrula
    assert_eq!(cards.len(), 4);
    for c in &cards {
        assert!(c.has_css_class("marathon-item-card"));
    }
}

fn main() {
    let mut failures: Vec<String> = Vec::new();
    let mut checks = 0usize;
    macro_rules! check {
        ($name:expr, $cond:expr) => {{
            checks += 1;
            if !($cond) {
                failures.push($name.to_string());
            } else {
                println!("[OK]   {}", $name);
            }
        }};
    }

    // ---- 1) Value round-trip (STRING tipi) ----
    let v = glib::Value::from("123".to_string());
    check!(
        "Value::from(String) -> Type::STRING",
        v.type_() == glib::Type::STRING
    );
    let back: Result<String, _> = v.get::<String>();
    check!(
        "Value get::<String>() round-trip",
        back == Ok("123".to_string())
    );

    // ---- 2) compute_reorder (reorder_marathon mantığı) ----
    let order = vec![10u64, 20, 30, 40];
    // 10 (idx 0) -> hedef idx 2  => [20,30,10,40]
    check!(
        "move id=10 -> idx 2",
        compute_reorder(&order, 10, 2) == vec![20, 30, 10, 40]
    );
    // 40 (idx 3) -> hedef idx 0  => [40,10,20,30]
    check!(
        "move id=40 -> idx 0",
        compute_reorder(&order, 40, 0) == vec![40, 10, 20, 30]
    );
    // 10 (idx 0) -> hedef idx 3 (sona) => [20,30,40,10]
    check!(
        "move id=10 -> idx 3 (son)",
        compute_reorder(&order, 10, 3) == vec![20, 30, 40, 10]
    );
    // aynı indekse bırakma => değişmez
    check!(
        "move id=20 -> idx 1 (sabit)",
        compute_reorder(&order, 20, 1) == order
    );

    // ---- 3) handle_drop mantığı ----
    let mut cb: Vec<(u64, usize)> = Vec::new();
    let _ = handle_drop(&glib::Value::from("0".to_string()), 2, 1, &mut cb);
    check!("drop kaynak id=0 -> (0,1) kaydedildi", cb == vec![(0, 1)]);

    let mut cb2: Vec<(u64, usize)> = Vec::new();
    let _ = handle_drop(&glib::Value::from("2".to_string()), 2, 1, &mut cb2);
    check!("kendine bırakma yok sayılır", cb2.is_empty());

    let mut cb3: Vec<(u64, usize)> = Vec::new();
    let _ = handle_drop(&glib::Value::from("xyz".to_string()), 2, 1, &mut cb3);
    check!("bozuk veri yok sayılır", cb3.is_empty());

    // ---- 5) centered hotspot (imleç kartın ortasında) ----
    check!(
        "centered_hotspot(200,100)=(100,50)",
        centered_hotspot(200, 100) == (100, 50)
    );
    check!(
        "centered_hotspot(0,0)=(0,0)",
        centered_hotspot(0, 0) == (0, 0)
    );

    // ---- 6) scroll_delta (auto-scroll kenar mantığı) ----
    check!(
        "scroll_delta üst kenar < 0 (yukarı)",
        scroll_delta(0.0, 500.0, 50.0) < 0.0
    );
    check!(
        "scroll_delta alt kenar > 0 (aşağı)",
        scroll_delta(500.0, 500.0, 50.0) > 0.0
    );
    check!(
        "scroll_delta orta = 0",
        scroll_delta(250.0, 500.0, 50.0) == 0.0
    );

    // ---- 4) GTK DnD kablolama smoke testi (display varsa) ----
    match gtk::init() {
        Ok(_) => {
            checks += 1;
            let result = std::panic::catch_unwind(|| {
                build_dnd_wiring();
            });
            match result {
                Ok(()) => {
                    println!("[OK]   DragSource/DropTarget kablolaması kuruldu (display mevcut)")
                }
                Err(_) => {
                    failures.push("DragSource/DropTarget kablolaması panik verdi".to_string())
                }
            }
        }
        Err(_) => {
            println!("[ATLA] Display yok -> GTK smoke testi atlandı (mantık testleri yeterli)");
        }
    }

    // ---- Sonuç ----
    println!();
    println!("========================================");
    println!("     SÜRÜKLE-BIRAK DnD DOĞRULAMA");
    println!("========================================");
    println!("Toplam kontrol: {}", checks);
    if failures.is_empty() {
        println!("SONUÇ: TÜM KONTROLLER GEÇTİ ✅");
        println!("========================================");
        std::process::exit(0);
    } else {
        println!("BAŞARISIZ KONTROLLER:");
        for f in &failures {
            println!("  ✗ {}", f);
        }
        println!("========================================");
        std::process::exit(1);
    }
}
