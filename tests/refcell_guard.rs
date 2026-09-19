//! RefCell guard yaşam süresi regresyon testleri (görüntü yok, GTK init gerekmez).
//!
//! Arka plan: `if let Some(x) = cell.borrow().metot()` deseninde borrow guard'ı
//! `if let` gövdesi boyunca yaşar; gövde içinde aynı hücreye `borrow_mut()`
//! panic atar ("already borrowed"). GTK sinyal trampolini (`extern "C"`,
//! nounwind) içinden yükselen bu panic unwind yapamaz → SIGABRT.
//! Bkz: kapak kalite değişiminde ComboRow `selected_notify` çökmesi.

use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn buggy_iflet_holds_guard_and_panics() {
    // Eski hatalı desenin gerçekten panic attığını belgele (düzeltme kanıtı).
    let h = Rc::new(RefCell::new(vec!["a", "b"]));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(_c) = h.borrow().last().cloned() {
            h.borrow_mut().pop();
        }
    }));
    assert!(r.is_err(), "if-let guard gövdede yaşamalı ve panic atmalı");
}

#[test]
fn fixed_clone_then_drop_allows_mut() {
    // Doğru desen: guard `;`'da düşer, gövdede borrow_mut güvenlidir.
    let h = Rc::new(RefCell::new(vec!["a", "b"]));
    let cur = h.borrow().last().cloned();
    if let Some(_c) = cur {
        h.borrow_mut().pop();
    }
    assert_eq!(*h.borrow(), ["a"]);
}
