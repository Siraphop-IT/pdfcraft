//! Help ▸ Check for updates, switched off: BMU PDF makes no network requests, so it has no update
//! check (at start or on request), no self-update and no `craft_update` dependency. The type and
//! entry points stay so upstream's callers keep compiling across merges.

use crate::PdfCraftApp;

#[derive(Default)]
pub(crate) struct Updates {
    /// The Updates dialog is showing (never, in BMU PDF).
    pub(crate) open: bool,
}

impl PdfCraftApp {
    /// Help ▸ Check for updates: nothing to ask and nothing opens.
    pub fn check_for_updates(&mut self) {}

    /// Called each frame; there is no check to poll.
    pub(crate) fn poll_updates(&mut self) {}
}

/// The Updates dialog: never shown.
pub(crate) fn dialog(_app: &mut PdfCraftApp, _ctx: &egui::Context) {}
