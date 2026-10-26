//! BMU PDF has no update check (issue #28 upstream): no Help ▸ Check for updates command, no check
//! at start, and nothing opens or asks the network when the entry point is called.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

#[test]
fn bmu_pdf_has_no_update_check() {
    assert!(pdfcraft_engine::commands::command("help.check_updates").is_none(), "no menu entry");
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        // Settings from a build that had the startup option are ignored.
        app.restore(r#"{"check_updates_at_start": true}"#);
        app
    });
    h.run_steps(3);
    h.state_mut().check_for_updates();
    h.run_steps(3);
    assert_eq!(h.state().last_opened_url, None, "no release page opens");
    assert!(h.query_by_label_contains("Checking for a newer version").is_none());
    assert!(h.query_by_label_contains("Check for updates").is_none());
}
