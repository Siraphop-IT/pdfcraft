//! Help ▸ Check for updates and the check at start, for BMU PDF's updates from the office NAS.
//! The desktop app supplies the [`Updater`](crate::Updater) (finding, downloading, checking and
//! installing a release); this module runs it off the UI thread and asks the person: a newer
//! version installs only when they choose Update now, with every document saved and no other
//! BMU PDF window open, and the app then closes so Windows Installer can replace it. Without an
//! updater (other builds, the web) there is no check and the command is hidden.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Align, Layout};

use crate::hooks::UpdateOffer;
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, widgets};

#[derive(Default)]
pub(crate) struct Updates {
    /// The Updates dialog is showing.
    pub(crate) open: bool,
    state: State,
    /// The check at start has been made (or wasn't wanted).
    started: bool,
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    /// Looking for a newer version; `manual` when Help ▸ Check for updates asked, which shows the
    /// answer whatever it is (the check at start speaks only of a newer version).
    Checking {
        reply: Receiver<Result<Option<UpdateOffer>, String>>,
        manual: bool,
    },
    UpToDate,
    Failed(String),
    Offer(UpdateOffer),
    Downloading {
        offer: UpdateOffer,
        progress: Arc<Progress>,
        reply: Receiver<Result<(), String>>,
    },
    /// The installer has started; the app is closing.
    Installing,
}

#[derive(Default)]
struct Progress {
    done: AtomicU64,
    total: AtomicU64,
}

impl PdfCraftApp {
    /// Help ▸ Check for updates: look now and say what was found.
    pub fn check_for_updates(&mut self) {
        if matches!(self.updates.state, State::Checking { .. } | State::Downloading { .. } | State::Installing) {
            self.updates.open = true;
            return;
        }
        self.start_check(true);
    }

    fn start_check(&mut self, manual: bool) {
        let Some(check) = self.updater.as_ref().map(|u| Arc::clone(&u.check)) else { return };
        let (send, reply) = channel();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new().name("update-check".into()).spawn(move || {
            let _ = send.send(check());
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        match spawned {
            Ok(_) => {
                self.updates.state = State::Checking { reply, manual };
                self.updates.open = manual;
            }
            Err(e) if manual => {
                self.updates.state = State::Failed(e.to_string());
                self.updates.open = true;
            }
            Err(e) => log::info!("update check not started: {e}"),
        }
    }

    /// Called each frame: start the check at start, and take in what the background work found.
    pub(crate) fn poll_updates(&mut self, ctx: &egui::Context) {
        if !self.updates.started {
            self.updates.started = true;
            if self.updater.as_ref().is_some_and(|u| u.at_start) {
                self.start_check(false);
            }
        }
        match &self.updates.state {
            State::Checking { reply, manual } => {
                let manual = *manual;
                let found = match reply.try_recv() {
                    Ok(found) => found,
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => Err(tl!("The update check stopped unexpectedly.").to_owned()),
                };
                self.updates.state = match found {
                    Ok(Some(offer)) => State::Offer(offer),
                    Ok(None) if manual => State::UpToDate,
                    Err(e) if manual => State::Failed(e),
                    Ok(None) => State::Idle,
                    Err(e) => {
                        log::info!("update check at start: {e}");
                        State::Idle
                    }
                };
                self.updates.open = !matches!(self.updates.state, State::Idle);
            }
            State::Downloading { reply, .. } => {
                let installed = match reply.try_recv() {
                    Ok(installed) => installed,
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => Err(tl!("The update stopped unexpectedly.").to_owned()),
                };
                match installed {
                    Ok(()) => {
                        self.updates.state = State::Installing;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(e) => {
                        log::warn!("update: {e}");
                        self.updates.state = State::Failed(e);
                    }
                }
            }
            _ => {}
        }
    }

    /// Why Update now can't run yet, if it can't: unsaved documents, or other windows open.
    fn update_blocked(&self) -> Option<String> {
        let unsaved = (0..self.views.len()).filter(|&i| self.has_unsaved_work(i)).count();
        if unsaved > 0 {
            return Some(crate::i18n::fmt(tl!("Save your documents first: {count} have unsaved changes."), &[("count", &unsaved.to_string())]));
        }
        let others = self.updater.as_ref().map_or(0, |u| (u.other_windows)());
        (others > 0).then(|| crate::i18n::fmt(tl!("Close BMU PDF's other windows first ({count} open)."), &[("count", &others.to_string())]))
    }

    fn start_install(&mut self, offer: UpdateOffer) {
        let Some(install) = self.updater.as_ref().map(|u| Arc::clone(&u.install)) else { return };
        let (send, reply) = channel();
        let progress = Arc::new(Progress::default());
        let shown = Arc::clone(&progress);
        let ctx = self.ctx.clone();
        let wanted = offer.clone();
        let spawned = std::thread::Builder::new().name("update-install".into()).spawn(move || {
            let tell = |done: u64, total: u64| {
                shown.done.store(done, Ordering::Relaxed);
                shown.total.store(total, Ordering::Relaxed);
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            };
            let _ = send.send(install(&wanted, &tell));
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        });
        self.updates.state = match spawned {
            Ok(_) => State::Downloading { offer, progress, reply },
            Err(e) => State::Failed(e.to_string()),
        };
    }
}

/// The Updates dialog.
pub(crate) fn dialog(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if !app.updates.open {
        return;
    }
    let t = Tokens::get(ctx);
    let current = app.updater.as_ref().map(|u| u.version.clone()).unwrap_or_default();
    let blocked = app.update_blocked();
    let mut close = false;
    let mut install = None;
    let modal = widgets::show_modal(ctx, "updates", |ui| {
        ui.set_width(460.0);
        let heading = |ui: &mut egui::Ui, text: &str| {
            ui.horizontal(|ui| {
                ui.add(crate::icons::image("cloud", 22.0, t.accent));
                ui.label(egui::RichText::new(text).font(theme::semibold(16.0)));
            });
            ui.add_space(6.0);
        };
        match &app.updates.state {
            State::Idle | State::Checking { .. } => {
                heading(ui, tl!("Check for updates"));
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tl!("Checking for a newer version…"));
                });
            }
            State::UpToDate => {
                heading(ui, tl!("BMU PDF is up to date"));
                ui.label(crate::i18n::fmt(tl!("Version {version} is the newest."), &[("version", &current)]));
            }
            State::Failed(error) => {
                heading(ui, tl!("Couldn't update BMU PDF"));
                ui.add(egui::Label::new(error.as_str()).wrap());
            }
            State::Offer(offer) => {
                heading(ui, tl!("A new version of BMU PDF is available"));
                ui.label(crate::i18n::fmt(
                    tl!("Version {version} is ready to install (you have {current})."),
                    &[("version", &offer.version), ("current", &current)],
                ));
                if !offer.notes.trim().is_empty() {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        ui.add(egui::Label::new(offer.notes.as_str()).wrap());
                    });
                }
                ui.add_space(6.0);
                ui.add(egui::Label::new(tl!("BMU PDF closes while it installs and opens again when it's done.")).wrap());
                if let Some(why) = &blocked {
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(egui::RichText::new(why).color(egui::Color32::from_rgb(0xD1, 0x3B, 0x3B))).wrap());
                }
            }
            State::Downloading { offer, progress, .. } => {
                heading(ui, &crate::i18n::fmt(tl!("Downloading BMU PDF {version}…"), &[("version", &offer.version)]));
                let total = progress.total.load(Ordering::Relaxed);
                let done = progress.done.load(Ordering::Relaxed);
                #[allow(clippy::cast_precision_loss)]
                let fraction = if total > 0 { (done as f64 / total as f64).clamp(0.0, 1.0) as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).show_percentage());
            }
            State::Installing => {
                heading(ui, tl!("Installing the update…"));
                ui.label(tl!("BMU PDF closes while it installs and opens again when it's done."));
            }
        }
        ui.add_space(14.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &app.updates.state {
            State::Offer(offer) => {
                if ui.add_enabled_ui(blocked.is_none(), |ui| widgets::pill_button(ui, tl!("Update now"), true)).inner.clicked() {
                    install = Some(offer.clone());
                }
                if widgets::pill_button(ui, tl!("Later"), false).clicked() {
                    close = true;
                }
            }
            State::Downloading { .. } | State::Installing => {}
            _ => {
                if widgets::pill_button(ui, tl!("OK"), true).clicked() {
                    close = true;
                }
            }
        });
    });
    let busy = matches!(app.updates.state, State::Downloading { .. } | State::Installing);
    if !busy && modal.should_close() {
        close = true;
    }
    if let Some(offer) = install {
        app.start_install(offer);
    } else if close {
        app.updates.open = false;
        // A check still running keeps going and opens the dialog again only for a newer version.
        if let State::Checking { manual, .. } = &mut app.updates.state {
            *manual = false;
        } else {
            app.updates.state = State::Idle;
        }
    }
}
