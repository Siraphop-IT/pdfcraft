//! Two documents side by side in one window (BMU PDF; Acrobat's Window ▸ Split, vertically): each
//! half is the page view of its own tab, scrolled, zoomed and turned on its own. The half last
//! pressed is the active tab, so the tools, side panels and keyboard act on it while the other
//! keeps its place. Choosing a tab that is not one of the two shows it alone, and choosing one of
//! them again brings the pair back; closing either ends it. A comparison shown side by side
//! (Compare files) takes precedence.

use egui::{Event, PointerButton};
use pdfcraft_engine::DocId;

use crate::PdfCraftApp;
use crate::theme::{self, Tokens};

impl PdfCraftApp {
    /// The two halves (left, right) as view indexes, when the active tab is one of a pair whose
    /// tabs are both still open.
    pub(crate) fn side_by_side_views(&self) -> Option<(usize, usize)> {
        let [left, right] = self.side_by_side?;
        let active = self.active?;
        let l = self.views.iter().position(|v| v.id == left)?;
        let r = self.views.iter().position(|v| v.id == right)?;
        (l != r && (active == l || active == r)).then_some((l, r))
    }

    /// Whether two documents show side by side now.
    pub fn side_by_side_showing(&self) -> bool {
        self.side_by_side_views().is_some()
    }

    /// Window ▸ Show side by side: ends the pair when it shows. Otherwise the active tab goes on
    /// the left and the tab after it (before it, for the last tab) on the right; with one tab
    /// open, a second PDF is chosen and opened beside it.
    pub fn toggle_side_by_side(&mut self) {
        if self.side_by_side_showing() {
            self.side_by_side = None;
            return;
        }
        let Some((index, id)) = self.active_ids() else { return };
        let other = if index.saturating_add(1) < self.views.len() { Some(index.saturating_add(1)) } else { index.checked_sub(1) };
        match other.and_then(|i| self.views.get(i)).map(|v| v.id) {
            Some(right) => self.show_side_by_side(id, right),
            None => self.open_beside(id),
        }
    }

    /// Shows tab `right` beside tab `left`, which becomes the active one. Nothing happens unless
    /// both are open and they differ.
    pub fn show_side_by_side(&mut self, left: DocId, right: DocId) {
        let Some(l) = self.views.iter().position(|v| v.id == left) else { return };
        if left == right || !self.views.iter().any(|v| v.id == right) {
            return;
        }
        self.side_by_side = Some([left, right]);
        self.active = Some(l);
    }

    /// Chooses a PDF and opens it to the right of `left`. A file already open is shown from its
    /// tab; a cancelled or failed open leaves the view as it was.
    fn open_beside(&mut self, left: DocId) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = rfd::AsyncFileDialog::new().add_filter("PDF", &["pdf"]);
            self.ask(crate::pickers::Ask::File(dialog), None, move |app, paths| {
                let Some(path) = paths.first().map(|p| p.to_string_lossy().into_owned()) else { return };
                // The tab may come later: from the background open, or after its password.
                if !matches!(app.open_path(&path), crate::OpenOutcome::Failed) {
                    app.side_by_side_waiting = Some((left, path));
                    app.poll_side_by_side();
                }
            });
        }
        // The browser build opens files through its own picker; open a second tab first.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = left;
            self.notify_tr("Open another PDF first, then show the two side by side.");
        }
    }
}

impl PdfCraftApp {
    /// Pairs a PDF chosen in [`Self::toggle_side_by_side`] once its tab is open (each frame). A
    /// file that is no longer being opened, and has no tab, was cancelled or failed: forgotten.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn poll_side_by_side(&mut self) {
        let Some((left, path)) = self.side_by_side_waiting.clone() else { return };
        if let Some(right) = self.tab_showing(&path).and_then(|i| self.views.get(i)).map(|v| v.id) {
            self.side_by_side_waiting = None;
            self.show_side_by_side(left, right);
        } else if self.password_prompt.is_none() && !self.pending_opens.iter().any(|p| p.path.as_deref().is_some_and(|p| crate::same_file(p, &path)))
        {
            self.side_by_side_waiting = None;
        }
    }
}

/// The caption row above each half: the file's name, with the active half marked.
const CAPTION_H: f32 = 24.0;

/// Draws `left` and `right` (view indexes) beside each other in `ui`. A press in a half makes its
/// tab the active one before either is drawn, so the press reaches that half's tools.
pub(crate) fn area(app: &mut PdfCraftApp, left: usize, right: usize, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let rect = ui.available_rect_before_wrap();
    let gap = 1.0;
    let half_w = ((rect.width() - gap) / 2.0).max(1.0);
    let halves = [
        egui::Rect::from_min_size(rect.min, egui::vec2(half_w, rect.height())),
        egui::Rect::from_min_size(egui::pos2(rect.min.x + half_w + gap, rect.min.y), egui::vec2(half_w, rect.height())),
    ];
    let ids = [app.views[left].id, app.views[right].id];
    // Only a press on the pages' own layer counts: one on a menu, popup or dialog over a half
    // belongs to that.
    let layer = ui.layer_id();
    let pressed = ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, .. } => Some(*pos),
            _ => None,
        })
    });
    if let Some(pos) = pressed.filter(|p| ui.ctx().layer_id_at(*p) == Some(layer))
        && let Some(k) = halves.iter().position(|h| h.contains(pos))
        && let Some(i) = ids.get(k).and_then(|id| app.views.iter().position(|v| v.id == *id))
    {
        app.active = Some(i);
    }
    let names = ids.map(|id| app.session.get(id).map(|d| d.display_name()).unwrap_or_default());
    ui.painter().vline(rect.min.x + half_w, rect.y_range(), egui::Stroke::new(gap, t.border));
    let mut close = false;
    for (k, half) in halves.into_iter().enumerate() {
        // Drawing a half can close its own tab (its snapshot bar's Close), which shifts the
        // indexes: each half is found again by id, and the frame stops once either is gone.
        let (Some(l), Some(r)) = (find(app, ids[0]), find(app, ids[1])) else { return };
        let index = if k == 0 { l } else { r };
        let active = app.active == Some(index);
        let caption = egui::Rect::from_min_size(half.min, egui::vec2(half.width(), CAPTION_H));
        ui.painter().rect_filled(caption, 0.0, t.panel);
        if active {
            let line = egui::Rect::from_min_max(egui::pos2(caption.min.x, caption.max.y - 2.0), caption.max);
            ui.painter().rect_filled(line, 0.0, t.accent);
        }
        let mut caption_ui =
            ui.new_child(egui::UiBuilder::new().max_rect(caption.shrink2(egui::vec2(8.0, 0.0))).id_salt(("side-by-side-caption", k)));
        caption_ui.horizontal_centered(|ui| {
            let color = if active { t.text } else { t.text_muted };
            let font = if active { theme::semibold(12.0) } else { theme::regular(12.0) };
            ui.add(egui::Label::new(egui::RichText::new(&names[k]).font(font).color(color)).truncate());
            if k == 1 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::icons::button(ui, "x", 20.0, false, tl!("Stop showing side by side")).clicked() {
                        close = true;
                    }
                });
            }
        });
        let body = egui::Rect::from_min_max(egui::pos2(half.min.x, half.min.y + CAPTION_H), half.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt(("side-by-side-half", k)));
        child.set_clip_rect(body);
        crate::canvas::document_area(app, index, &mut child);
    }
    if close {
        app.side_by_side = None;
    }
}

fn find(app: &PdfCraftApp, id: DocId) -> Option<usize> {
    app.views.iter().position(|v| v.id == id)
}
