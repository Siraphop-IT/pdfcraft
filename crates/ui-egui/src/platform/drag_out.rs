//! Pages dragged out of the window become a PDF file wherever they are dropped (Finder, File
//! Explorer, the desktop, a mail message). The file is written to a folder of its own under the
//! system's temporary folder, and the system's drag carries it from there: the place it is
//! dropped on copies it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Whether this system can carry a file out of the window: macOS and Windows. Elsewhere the
/// system's drag needs a GTK window, which the app's window isn't, and pages dragged out of the
/// window do what they did before (nothing).
pub const SUPPORTED: bool = cfg!(any(target_os = "macos", windows));

/// The folder under the system's temporary folder that dragged files are written in.
const FOLDER: &str = "pdfcraft-dragged-pages";

/// How long a dragged file is kept. The place it is dropped on copies it at once; a day leaves
/// time for a slow copy, and the next drag clears what is older.
const KEEP: Duration = Duration::from_secs(24 * 60 * 60);

/// Write `bytes` as `name` in a new folder of its own (so it keeps its name even while an earlier
/// drag's file of that name is still there) and return its path. `name` must be a plain file
/// name. Folders of earlier drags older than [`KEEP`] are removed first, as far as they can be.
pub fn write(name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    write_in(&std::env::temp_dir().join(FOLDER), name, bytes)
}

fn write_in(root: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    sweep(root, KEEP);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = root.join(format!("{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// Remove the folders in `root` last changed more than `keep` ago. Best effort: a folder that
/// can't be read or removed stays for the next time.
fn sweep(root: &Path, keep: Duration) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > keep);
        if old && entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Start the system's drag of the file at `path` out of the app's window, with a sheet (two for
/// several pages) under the pointer. On macOS it returns at once and the drag goes on without the
/// app; on Windows it returns when the file is dropped or the drag is cancelled.
#[cfg(any(target_os = "macos", windows))]
pub fn start(window: &eframe::Frame, path: PathBuf, pages: usize) -> Result<(), String> {
    let image = drag::Image::Raw(preview_png(pages));
    let options = drag::Options { skip_animatation_on_cancel_or_failure: false, mode: drag::DragMode::Copy };
    // The crate unwraps a few system results (an NSImage, a shell data object) it expects to get;
    // a refusal there must not take the app down.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drag::start_drag(window, drag::DragItem::Files(vec![path]), image, |_, _| {}, options)))
        .map_err(|_| "the system refused the drag".to_string())?
        .map_err(|e| e.to_string())
}

/// Not on this system ([`SUPPORTED`] is false): nothing to start.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn start(_window: &eframe::Frame, _path: PathBuf, _pages: usize) -> Result<(), String> {
    Err("dragging files out of the window is not supported on this system".to_string())
}

/// The picture under the pointer while the file is dragged: a blank sheet, or two overlapping
/// sheets for several pages, as a PNG.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn preview_png(pages: usize) -> Vec<u8> {
    const W: usize = 64;
    const H: usize = 80;
    const SHEET_W: usize = 52;
    const SHEET_H: usize = 68;
    let mut rgba = vec![0u8; W * H * 4];
    let mut sheet = |x0: usize, y0: usize| {
        for y in y0..(y0 + SHEET_H).min(H) {
            for x in x0..(x0 + SHEET_W).min(W) {
                let edge = x == x0 || y == y0 || x + 1 == x0 + SHEET_W || y + 1 == y0 + SHEET_H;
                let px = if edge { [120, 124, 130, 255] } else { [255, 255, 255, 255] };
                if let Some(dst) = rgba.get_mut((y * W + x) * 4..(y * W + x) * 4 + 4) {
                    dst.copy_from_slice(&px);
                }
            }
        }
    };
    if pages > 1 {
        sheet(W - SHEET_W, 0);
        sheet(0, H - SHEET_H);
    } else {
        sheet((W - SHEET_W) / 2, (H - SHEET_H) / 2);
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, W as u32, H as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    // Writing a fixed-size image into a Vec can't fail; an empty picture would only lose the
    // sheet under the pointer, never the drag.
    if let Ok(mut w) = enc.write_header() {
        let _ = w.write_image_data(&rgba);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{preview_png, sweep, write_in};
    use std::time::Duration;

    #[test]
    fn the_preview_is_a_png_the_system_can_read() {
        for pages in [1, 2, 40] {
            let png = preview_png(pages);
            let decoder = png::Decoder::new(std::io::Cursor::new(png));
            let reader = decoder.read_info().expect("a PNG");
            assert_eq!((reader.info().width, reader.info().height), (64, 80));
        }
    }

    #[test]
    fn each_drag_gets_its_own_folder_and_old_ones_are_cleared() {
        let root = std::env::temp_dir().join(format!("pdfcraft-drag-out-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let a = write_in(&root, "doc (page 1).pdf", b"%PDF-a").unwrap();
        let b = write_in(&root, "doc (page 1).pdf", b"%PDF-b").unwrap();
        assert_ne!(a, b, "the same name twice keeps both files");
        assert_eq!(std::fs::read(&a).unwrap(), b"%PDF-a");
        assert_eq!(std::fs::read(&b).unwrap(), b"%PDF-b");
        assert_eq!(b.file_name().unwrap(), "doc (page 1).pdf");
        sweep(&root, Duration::from_secs(3600));
        assert!(a.exists() && b.exists(), "recent drags stay");
        std::thread::sleep(Duration::from_millis(20));
        sweep(&root, Duration::from_millis(10));
        assert!(!a.exists() && !b.exists(), "old drags go");
        let _ = std::fs::remove_dir_all(&root);
    }
}
