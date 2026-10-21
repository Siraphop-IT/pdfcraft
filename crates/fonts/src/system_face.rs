//! BMU PDF: a face installed on the machine (Tahoma, Leelawadee UI, TH Sarabun New…) for the text
//! of comments and Fill & Sign text that the standard fonts can't draw, Thai first. The files are
//! read once, on first need, and embedded whole like the installed fonts Add text offers
//! ([`crate::embedding::validate_document_input`] explains the owner's decision).

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// A face read from the machine, with the family name its program gives.
pub struct SystemFace {
    pub bytes: Arc<Vec<u8>>,
    pub family: String,
}

/// Font files bigger than this are not read (Tahoma is under 1 MiB, Leelawadee UI under 2).
const MAX_FACE: u64 = 32 << 20;

/// The faces tried, best first: Tahoma reads well at the small sizes forms and comments use.
fn candidates() -> Vec<PathBuf> {
    let files = ["tahoma.ttf", "Tahoma.ttf", "LeelawUI.ttf", "leelawad.ttf", "THSarabunNew.ttf", "THSarabun.ttf"];
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        dirs.extend(std::env::var_os("WINDIR").map(|w| PathBuf::from(w).join("Fonts")));
        dirs.extend(std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Microsoft/Windows/Fonts")));
    } else if cfg!(target_os = "macos") {
        dirs.extend([PathBuf::from("/System/Library/Fonts/Supplemental"), PathBuf::from("/Library/Fonts")]);
        dirs.extend(std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Fonts")));
    }
    files.iter().flat_map(|f| dirs.iter().map(move |d| d.join(f))).collect()
}

fn load(path: &PathBuf) -> Option<SystemFace> {
    if std::fs::metadata(path).ok()?.len() > MAX_FACE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    crate::embedding::validate_document_input(&bytes).ok()?;
    let (family, _) = crate::embedding::names(&bytes).ok()?;
    Some(SystemFace { bytes: Arc::new(bytes), family })
}

/// The first installed face that has a glyph for every character of `text`, if any.
pub fn for_text(text: &str) -> Option<&'static SystemFace> {
    static FACES: OnceLock<Vec<SystemFace>> = OnceLock::new();
    let faces = FACES.get_or_init(|| {
        let mut seen = Vec::new();
        candidates()
            .iter()
            .filter_map(load)
            // Case-insensitive file systems list the same file under two names.
            .filter(|f| {
                let new = !seen.contains(&f.family);
                seen.push(f.family.clone());
                new
            })
            .collect()
    });
    faces.iter().find(|f| crate::embedding::covers(&f.bytes, text))
}
