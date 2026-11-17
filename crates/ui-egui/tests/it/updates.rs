//! BMU PDF's updates from the office NAS (ui-egui `updates.rs`), with a stand-in for the desktop
//! app's [`Updater`], so nothing here reaches the network: Help ▸ Check for updates only where
//! there is an updater, a newer version is offered (at start, or when asked) and installs only
//! when Update now is pressed with every document saved and no other window open, and the app
//! then closes so the installer can replace it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::{PdfCraftApp, UpdateOffer, Updater};

use crate::support::{menu_row, open_menu, step_until};

/// What the stand-in updater finds and was asked to do.
#[derive(Default)]
struct Fake {
    newest: Mutex<Option<UpdateOffer>>,
    checks: AtomicUsize,
    installs: Mutex<Vec<String>>,
    other_windows: AtomicUsize,
}

fn offer() -> UpdateOffer {
    UpdateOffer { version: "0.18.42".into(), notes: "Tabs can be dragged into another window.".into() }
}

fn updater(fake: &Arc<Fake>, at_start: bool) -> Updater {
    let (check, install, windows) = (Arc::clone(fake), Arc::clone(fake), Arc::clone(fake));
    Updater {
        version: "0.18.7".into(),
        check: Arc::new(move || {
            check.checks.fetch_add(1, Ordering::SeqCst);
            Ok(check.newest.lock().map(|n| n.clone()).unwrap_or_default())
        }),
        install: Arc::new(move |offer, progress| {
            progress(5, 10);
            install.installs.lock().map_err(|e| e.to_string())?.push(offer.version.clone());
            Ok(())
        }),
        other_windows: Box::new(move || windows.other_windows.load(Ordering::SeqCst)),
        at_start,
    }
}

fn harness(updater: Option<Updater>, unsaved: bool) -> Harness<'static, PdfCraftApp> {
    let mut slot = Some(updater);
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.updater = slot.take().flatten();
        if unsaved {
            app.open_bytes("doc.pdf", None, one_page_pdf()).expect("fixture opens");
            app.apply_edit(pdfcraft_engine::Edit::RotatePages { pages: vec![0], degrees: 90 });
        }
        app
    });
    h.run_steps(3);
    h
}

/// A one-page PDF.
fn one_page_pdf() -> Vec<u8> {
    let objects =
        ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 300] >>", "<< /Type /Page /Parent 2 0 R >>"];
    let mut out = b"%PDF-1.7
"
    .to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(
            format!(
                "{} 0 obj
{o}
endobj
",
                i + 1
            )
            .as_bytes(),
        );
    }
    let xref = out.len();
    out.extend_from_slice(
        format!(
            "xref
0 {}
0000000000 65535 f 
",
            objects.len() + 1
        )
        .as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(
            format!(
                "{o:010} 00000 n 
"
            )
            .as_bytes(),
        );
    }
    out.extend_from_slice(
        format!(
            "trailer
<< /Size {} /Root 1 0 R >>
startxref
{xref}
%%EOF
",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[test]
fn without_an_updater_there_is_no_update_check() {
    let mut h = harness(None, false);
    open_menu(&mut h, "Help");
    assert!(h.query_by_label("Check for updates…").is_none(), "no menu entry");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.state_mut().check_for_updates();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Checking for a newer version").is_none());
    assert!(!h.state_mut().execute("help.check_updates"), "the command is off");
}

#[test]
fn a_newer_version_found_at_start_installs_when_asked_and_the_app_closes() {
    let fake = Arc::new(Fake::default());
    *fake.newest.lock().unwrap() = Some(offer());
    let mut h = harness(Some(updater(&fake, true)), false);
    assert!(step_until(&mut h, |h| h.query_by_label("A new version of BMU PDF is available").is_some()));
    h.get_by_label_contains("Version 0.18.42 is ready to install (you have 0.18.7)");
    h.get_by_label("Tabs can be dragged into another window.");
    assert!(fake.installs.lock().unwrap().is_empty(), "nothing installs before it is asked");

    h.get_by_label("Update now").click();
    assert!(step_until(&mut h, |h| h.query_by_label("Installing the update…").is_some()));
    let closes = h.output().viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.commands.contains(&egui::ViewportCommand::Close));
    assert!(closes, "the app closes for the installer");
    assert_eq!(*fake.installs.lock().unwrap(), vec!["0.18.42".to_string()]);
    assert_eq!(fake.checks.load(Ordering::SeqCst), 1);
}

#[test]
fn later_leaves_the_newer_version_uninstalled() {
    let fake = Arc::new(Fake::default());
    *fake.newest.lock().unwrap() = Some(offer());
    let mut h = harness(Some(updater(&fake, true)), false);
    assert!(step_until(&mut h, |h| h.query_by_label("A new version of BMU PDF is available").is_some()));
    h.get_by_label("Later").click();
    h.run_steps(3);
    assert!(h.query_by_label("A new version of BMU PDF is available").is_none());
    assert!(fake.installs.lock().unwrap().is_empty());
}

#[test]
fn an_unsaved_document_holds_the_update_back() {
    let fake = Arc::new(Fake::default());
    *fake.newest.lock().unwrap() = Some(offer());
    let mut h = harness(Some(updater(&fake, true)), true);
    assert!(step_until(&mut h, |h| h.query_by_label("A new version of BMU PDF is available").is_some()));
    h.get_by_label_contains("Save your documents first: 1 have unsaved changes.");
    h.get_by_label("Update now").click();
    h.run_steps(5);
    assert!(fake.installs.lock().unwrap().is_empty(), "an unsaved document holds it back");
}

#[test]
fn other_windows_hold_the_update_back_until_they_close() {
    let fake = Arc::new(Fake::default());
    *fake.newest.lock().unwrap() = Some(offer());
    fake.other_windows.store(2, Ordering::SeqCst);
    let mut h = harness(Some(updater(&fake, true)), false);
    assert!(step_until(&mut h, |h| h.query_by_label("A new version of BMU PDF is available").is_some()));
    h.get_by_label_contains("Close BMU PDF's other windows first (2 open).");
    h.get_by_label("Update now").click();
    h.run_steps(5);
    assert!(fake.installs.lock().unwrap().is_empty(), "another window holds it back");

    fake.other_windows.store(0, Ordering::SeqCst);
    h.run_steps(3);
    h.get_by_label("Update now").click();
    assert!(step_until(&mut h, |h| h.query_by_label("Installing the update…").is_some()));
    assert_eq!(fake.installs.lock().unwrap().len(), 1);
}

#[test]
fn the_check_at_start_is_silent_unless_there_is_a_newer_version_and_asking_always_answers() {
    let fake = Arc::new(Fake::default());
    let mut h = harness(Some(updater(&fake, true)), false);
    assert!(step_until(&mut h, |_| fake.checks.load(Ordering::SeqCst) == 1));
    h.run_steps(5);
    assert!(h.query_by_label("BMU PDF is up to date").is_none(), "nothing to say at start");

    open_menu(&mut h, "Help");
    menu_row(&h, "Check for updates…").click();
    assert!(step_until(&mut h, |h| h.query_by_label("BMU PDF is up to date").is_some()));
    h.get_by_label("Version 0.18.7 is the newest.");
    assert_eq!(fake.checks.load(Ordering::SeqCst), 2);

    let quiet = Arc::new(Fake::default());
    let mut h = harness(Some(updater(&quiet, false)), false);
    h.run_steps(5);
    assert_eq!(quiet.checks.load(Ordering::SeqCst), 0, "no check at start when it isn't wanted");
}
