//! Author avatars, as in ungit: Gravatar (MD5 of the e-mail), plus GitHub avatars for GitHub
//! "noreply" addresses. Fetched off the UI thread and cached on disk; when there is none, the
//! UI falls back to the author's initials.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime};

use eframe::egui;

/// Pixel size requested and kept.
pub const SIZE: u32 = 80;

/// `Ok(Some(bytes))`: an image; `Ok(None)`: the user has no avatar; `Err`: try again later.
pub type Fetch = Arc<dyn Fn(&str) -> Result<Option<Vec<u8>>, String> + Send + Sync>;

/// Where to download the avatar of `email` from.
pub fn avatar_url(email: &str) -> String {
    let email = email.trim().to_lowercase();
    if let Some(user) = email.strip_suffix("@users.noreply.github.com") {
        return match user.split_once('+') {
            Some((id, _)) if id.bytes().all(|b| b.is_ascii_digit()) => {
                format!("https://avatars.githubusercontent.com/u/{id}?s={SIZE}")
            }
            _ => format!("https://github.com/{user}.png?size={SIZE}"),
        };
    }
    format!("https://www.gravatar.com/avatar/{}?s={SIZE}&d=404", md5_hex(email.as_bytes()))
}

fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Makes everything outside the inscribed circle transparent (antialiased edge).
pub fn circle_mask(img: &mut image::RgbaImage) {
    let (w, h) = img.dimensions();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let r = cx.min(cy);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
        let coverage = (r - d + 0.5).clamp(0.0, 1.0);
        px[3] = (f32::from(px[3]) * coverage).round() as u8;
    }
}

/// How long "this person has no avatar" is trusted before asking again.
const MISSING_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

/// Decodes any supported image, scales it to `SIZE`, masks it round and re-encodes it as PNG.
fn normalise(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let mut img = img.resize_to_fill(SIZE, SIZE, image::imageops::FilterType::Triangle).to_rgba8();
    circle_mask(&mut img);
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Disk cache first, then the network. Returns a normalised PNG or `None`.
fn load(dir: &std::path::Path, fetch: &Fetch, email: &str) -> Option<Vec<u8>> {
    let url = avatar_url(email);
    let key = md5_hex(url.as_bytes());
    let png = dir.join(format!("{key}.png"));
    let none = dir.join(format!("{key}.none"));
    if let Ok(bytes) = std::fs::read(&png) {
        return Some(bytes);
    }
    let fresh = |p: &PathBuf| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < MISSING_TTL)
    };
    if fresh(&none) {
        return None;
    }
    match fetch(&url) {
        Ok(Some(bytes)) => {
            let png_bytes = normalise(&bytes)?;
            let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&png, &png_bytes));
            Some(png_bytes)
        }
        Ok(None) => {
            let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&none, b""));
            None
        }
        Err(_) => None,
    }
}

/// Default network fetch: 404 means "no avatar".
pub fn http_fetch() -> Fetch {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
    Arc::new(move |url: &str| match agent.get(url).call() {
        Ok(mut resp) => resp
            .body_mut()
            .with_config()
            .limit(2 * 1024 * 1024)
            .read_to_vec()
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(e.to_string()),
    })
}

pub fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "gigit")
        .map(|d| d.cache_dir().join("avatars"))
        .unwrap_or_else(|| std::env::temp_dir().join("gigit-avatars"))
}

enum Slot {
    Loading,
    Ready(egui::TextureHandle),
    Missing,
}

pub struct AvatarStore {
    slots: HashMap<String, Slot>,
    requests: Sender<String>,
    results: Receiver<(String, Option<Vec<u8>>)>,
}

impl AvatarStore {
    /// Starts the download thread; it stops when the store is dropped.
    pub fn new(dir: PathBuf, fetch: Fetch) -> Self {
        let (requests, queue) = channel::<String>();
        let (tx, results) = channel();
        std::thread::Builder::new()
            .name("gigit-avatars".into())
            .spawn(move || {
                for email in queue {
                    let png = load(&dir, &fetch, &email);
                    if tx.send((email, png)).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn avatar thread");
        AvatarStore { slots: HashMap::new(), requests, results }
    }

    /// The avatar of `email` if it is loaded; otherwise starts loading it and returns `None`.
    pub fn get(&mut self, _ctx: &egui::Context, email: &str) -> Option<egui::TextureHandle> {
        match self.slots.get(email) {
            Some(Slot::Ready(t)) => Some(t.clone()),
            Some(_) => None,
            None => {
                self.slots.insert(email.to_string(), Slot::Loading);
                let _ = self.requests.send(email.to_string());
                None
            }
        }
    }

    /// Turns finished downloads into textures. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((email, png)) = self.results.try_recv() {
            let slot = png
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
                .map(|img| {
                    let img = img.to_rgba8();
                    let size = [img.width() as usize, img.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    Slot::Ready(ctx.load_texture(format!("avatar-{email}"), color, egui::TextureOptions::LINEAR))
                })
                .unwrap_or(Slot::Missing);
            self.slots.insert(email, slot);
        }
    }

    /// True while downloads are pending (the UI keeps repainting to show them).
    pub fn loading(&self) -> bool {
        self.slots.values().any(|s| matches!(s, Slot::Loading))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    #[test]
    fn gravatar_url_uses_md5_of_normalised_email() {
        assert_eq!(
            avatar_url("  Test@Example.COM "),
            "https://www.gravatar.com/avatar/55502f40dc8b7c769880b10874abc9d0?s=80&d=404"
        );
    }

    #[test]
    fn github_noreply_addresses_use_github_avatars() {
        assert_eq!(avatar_url("12345+octo@users.noreply.github.com"), "https://avatars.githubusercontent.com/u/12345?s=80");
        assert_eq!(avatar_url("octo@users.noreply.github.com"), "https://github.com/octo.png?size=80");
    }

    #[test]
    fn circle_mask_clears_corners_only() {
        let mut img = image::RgbaImage::from_pixel(80, 80, image::Rgba([200, 100, 50, 255]));
        circle_mask(&mut img);
        assert_eq!(img.get_pixel(0, 0)[3], 0);
        assert_eq!(img.get_pixel(79, 79)[3], 0);
        assert_eq!(img.get_pixel(40, 40)[3], 255);
        assert_eq!(img.get_pixel(40, 2)[3], 255);
    }

    fn png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 200, 30, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn wait_for(store: &mut AvatarStore, ctx: &egui::Context, email: &str) -> Option<egui::TextureHandle> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            store.poll(ctx);
            if let Some(t) = store.get(ctx, email) {
                return Some(t);
            }
            if matches!(store.slots.get(email), Some(Slot::Missing)) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("avatar never settled");
    }

    #[test]
    fn fetches_once_then_uses_the_disk_cache() {
        let dir = tempfile::TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let fetch: Fetch = Arc::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(Some(png_bytes()))
        });
        let ctx = egui::Context::default();
        let mut store = AvatarStore::new(dir.path().to_path_buf(), fetch.clone());
        assert!(store.get(&ctx, "a@b.c").is_none(), "first call only starts loading");
        let tex = wait_for(&mut store, &ctx, "a@b.c").expect("texture");
        assert_eq!(tex.size(), [SIZE as usize, SIZE as usize]);
        // A new store (next launch) reads the disk cache instead of the network.
        let mut again = AvatarStore::new(dir.path().to_path_buf(), fetch);
        assert!(wait_for(&mut again, &ctx, "a@b.c").is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn missing_avatar_is_remembered() {
        let dir = tempfile::TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let fetch: Fetch = Arc::new(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        });
        let ctx = egui::Context::default();
        let mut store = AvatarStore::new(dir.path().to_path_buf(), fetch.clone());
        assert!(wait_for(&mut store, &ctx, "none@b.c").is_none());
        let mut again = AvatarStore::new(dir.path().to_path_buf(), fetch);
        assert!(wait_for(&mut again, &ctx, "none@b.c").is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn network_errors_are_not_cached() {
        let dir = tempfile::TempDir::new().unwrap();
        let fetch: Fetch = Arc::new(|_| Err("offline".into()));
        let ctx = egui::Context::default();
        let mut store = AvatarStore::new(dir.path().to_path_buf(), fetch);
        assert!(wait_for(&mut store, &ctx, "x@y.z").is_none());
        assert_eq!(std::fs::read_dir(dir.path()).map(|d| d.count()).unwrap_or(0), 0);
    }
}
