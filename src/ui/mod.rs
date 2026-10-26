//! egui front-end.

mod dialogs;
mod sidebar;
pub mod state;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use eframe::egui;

use crate::config::Config;
use crate::theme::{self, Theme};
use crate::worker::Job;
use state::AppState;

pub struct GigitApp {
    pub state: AppState,
    ipc_rx: Receiver<Option<PathBuf>>,
    folder_tx: Sender<PathBuf>,
    folder_rx: Receiver<PathBuf>,
    applied: Option<(Theme, f32)>,
    window_size: Option<[f32; 2]>,
}

impl GigitApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Config,
        config_path: PathBuf,
        cli_path: Option<PathBuf>,
        ipc_rx: Receiver<Option<PathBuf>>,
    ) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let mut state = AppState::new(config, config_path, Arc::new(move || ctx.request_repaint()));
        match cli_path.or_else(|| state.config.last_open.clone()) {
            Some(p) => state.add_folder(p),
            None => state.refresh_badges(),
        }
        let (folder_tx, folder_rx) = channel();
        GigitApp { state, ipc_rx, folder_tx, folder_rx, applied: None, window_size: None }
    }

    /// Opens the native folder picker without blocking the UI.
    pub fn pick_folder(&self, ctx: &egui::Context) {
        let tx = self.folder_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                let _ = tx.send(p);
                ctx.request_repaint();
            }
        });
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let wanted = (self.state.theme, self.state.config.font_size);
        if self.applied != Some(wanted) {
            theme::apply(ctx, theme::palette(wanted.0), wanted.1);
            self.applied = Some(wanted);
        }
    }

    fn autofetch(&mut self, ctx: &egui::Context) {
        let minutes = self.state.config.autofetch_minutes;
        if minutes == 0 {
            return;
        }
        let interval = Duration::from_secs(u64::from(minutes) * 60);
        if let Some(v) = self.state.open.as_mut() {
            if v.last_fetch.elapsed() >= interval {
                v.last_fetch = std::time::Instant::now();
                v.worker.send(Job::Fetch { background: true });
            }
            ctx.request_repaint_after(interval.saturating_sub(v.last_fetch.elapsed()));
        }
    }
}

impl eframe::App for GigitApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.state.poll();
        while let Ok(p) = self.ipc_rx.try_recv() {
            if let Some(p) = p {
                self.state.add_folder(p);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        while let Ok(p) = self.folder_rx.try_recv() {
            self.state.add_folder(p);
        }
        self.apply_theme(ctx);
        self.autofetch(ctx);
        if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
            self.window_size = Some([r.width(), r.height()]);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        sidebar::show(self, ui);
        dialogs::error_banner(&mut self.state, ui);
        egui::CentralPanel::default().show(ui, |ui| {
            if self.state.open.is_none() {
                dialogs::empty_state(self, ui);
            }
        });
        dialogs::pending(&mut self.state, &ctx);
    }

    fn on_exit(&mut self) {
        self.state.config.window = self.window_size;
        self.state.save_config();
    }
}
