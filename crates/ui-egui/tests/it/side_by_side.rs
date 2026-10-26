//! BMU PDF: two documents side by side in one window (View ▸ Show side by side).

use crate::support;

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

fn pdf(width: u32) -> Vec<u8> {
    format!(
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} 400] >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF"
    )
    .into_bytes()
}

fn harness(files: &'static [&'static str]) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        for name in files {
            app.open_bytes(name, None, pdf(300)).unwrap();
        }
        app
    });
    h.run_steps(4);
    h
}

/// A press and release at the centre of view `index`'s page area.
fn click_view(h: &mut Harness<'static, PdfCraftApp>, index: usize) {
    let p = h.state().views[index].viewport_rect().center();
    let button = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    support::input::one_frame(h, [Event::PointerMoved(p), button(true)]);
    support::input::one_frame(h, [button(false)]);
    h.run_steps(2);
}

#[test]
fn two_open_tabs_show_side_by_side_and_the_pressed_half_is_active() {
    let mut h = harness(&["one.pdf", "two.pdf", "three.pdf"]);
    h.state_mut().active = Some(0);
    h.run_steps(2);
    assert!(h.state_mut().execute("view.side_by_side"));
    h.run_steps(3);
    assert!(h.state().side_by_side_showing());
    assert_eq!(h.state().active, Some(0), "the active tab goes on the left");
    // Both halves are drawn, side by side.
    let (left, right) = (h.state().views[0].viewport_rect(), h.state().views[1].viewport_rect());
    assert!(left.right() <= right.left() + 1.0 && left.width() > 300.0 && right.width() > 300.0, "{left:?} {right:?}");
    // A press in the right half makes its tab the active one; the pair stays.
    click_view(&mut h, 1);
    assert_eq!(h.state().active, Some(1));
    assert!(h.state().side_by_side_showing());
    // Another tab shows alone; coming back to one of the pair shows both again.
    h.state_mut().active = Some(2);
    h.run_steps(3);
    assert!(!h.state().side_by_side_showing());
    h.state_mut().active = Some(0);
    h.run_steps(3);
    assert!(h.state().side_by_side_showing());
    // The caption's close button ends it.
    h.get_by_label("Stop showing side by side").click();
    h.run_steps(3);
    assert!(!h.state().side_by_side_showing());
    // And the command toggles it on and off.
    assert!(h.state_mut().execute("view.side_by_side"));
    h.run_steps(2);
    assert!(h.state().side_by_side_showing());
    assert!(h.state_mut().execute("view.side_by_side"));
    h.run_steps(2);
    assert!(!h.state().side_by_side_showing());
}

#[test]
fn closing_either_tab_ends_the_pair() {
    let mut h = harness(&["one.pdf", "two.pdf"]);
    h.state_mut().active = Some(1);
    assert!(h.state_mut().execute("view.side_by_side"));
    h.run_steps(3);
    assert!(h.state().side_by_side_showing(), "the last tab pairs with the one before it");
    h.state_mut().close_tab(0);
    h.run_steps(3);
    assert!(!h.state().side_by_side_showing());
    assert_eq!(h.state().views.len(), 1);
}

#[test]
fn with_one_tab_open_a_second_pdf_is_chosen_and_opened_beside_it() {
    let dir = std::env::temp_dir().join(format!("bmu-side-by-side-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let second = dir.join("second.pdf");
    std::fs::write(&second, pdf(500)).unwrap();
    let mut h = harness(&["first.pdf"]);
    h.state_mut().pick_override = Some(vec![second.to_string_lossy().into_owned()]);
    assert!(h.state_mut().execute("view.side_by_side"));
    support::wait_until(&mut h, "the second PDF beside the first", |app| app.views.len() == 2 && app.side_by_side_showing());
    assert_eq!(h.state().active, Some(0), "the first stays active, on the left");
    std::fs::remove_dir_all(dir).unwrap();
}
