use std::path::PathBuf;
use std::sync::{Arc, OnceLock, mpsc::channel};

use eframe::egui;
use gigit::config::{Config, config_path};
use gigit::single_instance;
use gigit::ui::GigitApp;

fn main() -> eframe::Result<()> {
    let cli_path = std::env::args_os().nth(1).map(PathBuf::from).map(|p| std::path::absolute(&p).unwrap_or(p));
    let sock = single_instance::socket_path();
    if single_instance::try_forward(&sock, cli_path.as_deref()) {
        return Ok(());
    }

    let (ipc_tx, ipc_rx) = channel();
    let ctx_cell: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
    let cell = ctx_cell.clone();
    // If another instance won a race for the socket we still start, just without forwarding.
    let _ = single_instance::listen(&sock, move |p| {
        let _ = ipc_tx.send(p);
        if let Some(ctx) = cell.get() {
            ctx.request_repaint();
        }
    });

    let path = config_path();
    let config = Config::load(&path);
    let size = config.window.unwrap_or([1280.0, 800.0]);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("GiGit")
        .with_app_id("gigit")
        .with_inner_size(size)
        .with_min_inner_size([820.0, 520.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/gigit-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "GiGit",
        options,
        Box::new(move |cc| {
            let _ = ctx_cell.set(cc.egui_ctx.clone());
            Ok(Box::new(GigitApp::new(cc, config, path, cli_path, ipc_rx)))
        }),
    )
}
