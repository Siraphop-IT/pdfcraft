//! Explicit document-font discovery. Interface fallbacks never enter this catalogue.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct FontFile {
    pub path: PathBuf,
    pub family: String,
    pub face: String,
    /// The face draws Thai: pickers list these first.
    pub thai: bool,
}

/// Standard user-installed font folders; discovery is performed only on request.
pub fn installed_directories() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        // macOS's own Thai faces (Tahoma, Sathu, Silom…) are in the supplemental folder.
        let mut dirs = vec![PathBuf::from("/Library/Fonts"), PathBuf::from("/System/Library/Fonts/Supplemental")];
        if let Some(home) = home {
            dirs.push(home.join("Library/Fonts"));
        }
        dirs
    } else if cfg!(windows) {
        let mut dirs = std::env::var_os("WINDIR").map(|p| PathBuf::from(p).join("Fonts")).into_iter().collect::<Vec<_>>();
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
        }
        dirs
    } else {
        let mut dirs = vec![PathBuf::from("/usr/share/fonts"), PathBuf::from("/usr/local/share/fonts")];
        if let Some(home) = home {
            dirs.extend([home.join(".local/share/fonts"), home.join(".fonts")]);
        }
        dirs
    }
}

/// Bounded, symlink-free scan. Only standalone faces whose embedding permissions allow it are
/// listed (BMU PDF lists installed fonts whatever their licence: they are the user's own). The
/// caller controls roots (automation resolves confinement before calling this).
pub fn discover(roots: &[PathBuf]) -> Vec<FontFile> {
    use std::io::Read;
    let mut pending: Vec<_> = roots.iter().take(16).map(|r| (r.clone(), 0usize)).collect();
    let mut files = Vec::new();
    let mut entries = 0usize;
    // Windows' font folder holds several hundred megabytes; Thai faces are read first (below), so
    // they are listed even when the budget runs out.
    let mut budget = 512u64 << 20;
    let mut seen = std::collections::HashSet::new();
    while let Some((path, depth)) = pending.pop() {
        if entries >= 4096 || budget == 0 {
            break;
        }
        entries += 1;
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() || !seen.insert(path.clone()) {
            continue;
        }
        if meta.is_dir() && depth < 4 {
            if let Ok(children) = std::fs::read_dir(&path) {
                let remaining = 4096usize.saturating_sub(entries.saturating_add(pending.len()));
                let mut children: Vec<_> = children.take(remaining).filter_map(Result::ok).map(|p| (p.path(), depth + 1)).collect();
                // `pending` is a stack: what sorts last is read first.
                children.sort_by_key(|(p, _)| thai_face(p));
                pending.extend(children);
            }
        } else if meta.is_file() && meta.len() <= (16 << 20) && font_extension(&path) {
            if meta.len() > budget {
                continue;
            }
            budget = budget.saturating_sub(meta.len());
            let mut bytes = Vec::new();
            let read = std::fs::File::open(&path).and_then(|f| f.take((16 << 20) + 1).read_to_end(&mut bytes));
            if read.is_err()
                || pdfcraft_fonts::embedding::validate_document_input(&bytes).is_err()
                || pdfcraft_fonts::embedding::validate(&bytes).is_err()
            {
                continue;
            }
            if let Ok((family, face)) = pdfcraft_fonts::embedding::names(&bytes) {
                let thai = pdfcraft_fonts::embedding::covers(&bytes, "กขค");
                files.push(FontFile { path, family, face, thai });
            }
        }
    }
    files.sort_by(|a, b| b.thai.cmp(&a.thai).then(a.face.cmp(&b.face)).then(a.path.cmp(&b.path)));
    files
}
/// Whether `path` looks like a Thai face by its file name (Windows' and the Thai national fonts).
fn thai_face(path: &Path) -> bool {
    let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or_default().to_ascii_lowercase();
    [
        "angsa",
        "angsana",
        "browa",
        "browallia",
        "cordia",
        "dillenia",
        "eucrosia",
        "freesia",
        "iriss",
        "jasmine",
        "kodchiang",
        "lilyupc",
        "leelaw",
        "tahoma",
        "thsarabun",
        "th sarabun",
        "sarabun",
        "thniramit",
        "thkodchasal",
        "thchakra",
        "thbaijam",
        "thcharmonman",
        "thfahkwang",
        "thkrub",
        "thmalilt",
        "thsrisakdi",
        "thk2d",
    ]
    .iter()
    .any(|p| name.starts_with(p))
}

fn font_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
}
