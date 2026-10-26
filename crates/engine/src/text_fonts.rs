//! Explicit document-font discovery. Interface fallbacks never enter this catalogue.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFile {
    pub path: PathBuf,
    pub family: String,
    pub face: String,
    /// The face draws Thai: pickers list these first.
    pub thai: bool,
    /// Its face in a collection (`.ttc`: Windows keeps Angsana New and Cordia New in them); 0 for a
    /// single-face file.
    pub index: u32,
}

impl FontFile {
    /// The face's font program, standalone (taken out of its collection).
    pub fn read(&self) -> Result<Vec<u8>, String> {
        let bytes = std::fs::read(&self.path).map_err(|e| e.to_string())?;
        pdfcraft_fonts::embedding::standalone_face(&bytes, self.index)
    }
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
            if read.is_err() {
                continue;
            }
            for index in 0..pdfcraft_fonts::embedding::face_count(&bytes).min(32) {
                let Ok(face_bytes) = pdfcraft_fonts::embedding::standalone_face(&bytes, index) else { continue };
                if pdfcraft_fonts::embedding::validate_document_input(&face_bytes).is_err()
                    || pdfcraft_fonts::embedding::validate(&face_bytes).is_err()
                {
                    continue;
                }
                if let Ok((family, face)) = pdfcraft_fonts::embedding::names(&face_bytes) {
                    let thai = pdfcraft_fonts::embedding::covers(&face_bytes, "กขค");
                    files.push(FontFile { path: path.clone(), family, face, thai, index });
                }
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
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["ttf", "otf", "ttc"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The fonts installed on this machine, discovered once (BMU PDF's automatic Thai fallback).
fn installed() -> &'static [FontFile] {
    static FONTS: std::sync::OnceLock<Vec<FontFile>> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| discover(&installed_directories()))
}

/// BMU PDF: rewrite paragraph `block` of `page` in Thai its own font can't draw (a subset of
/// Cordia New, say), the way a Thai office expects rather than with an error: in the same family
/// installed on this machine, else in the bundled Thai face (Noto Sans Thai, when the build has
/// craft-fonts). The rewritten document, or `None` when neither can draw it.
pub(crate) fn thai_rewrite(
    doc: &pdfcraft_cos::Document,
    page: usize,
    block: usize,
    text: &str,
    style: &pdfcraft_edit::BlockStyle,
) -> Option<pdfcraft_cos::Document> {
    let source = pdfcraft_edit::text_blocks(doc, page).ok()?.into_iter().nth(block)?;
    let key = pdfcraft_fonts::embedding::face_key(&source.base_font);
    // The family's regular face first, then its others (a bold paragraph keeps its own style
    // through the style's bold flag where the face allows).
    let mut same: Vec<&FontFile> = installed().iter().filter(|f| pdfcraft_fonts::embedding::face_key(&f.family) == key).collect();
    same.sort_by_key(|f| f.face != f.family && !f.face.ends_with("Regular"));
    for font in same {
        let Ok(bytes) = font.read() else { continue };
        if !pdfcraft_fonts::embedding::covers(&bytes, text) {
            continue;
        }
        let mut attempt = doc.clone();
        let Ok(resource) = pdfcraft_edit::text::register_text_font(&mut attempt, page, &bytes) else { continue };
        let chosen = pdfcraft_edit::BlockStyle { resource: Some(resource), ..style.clone() };
        if pdfcraft_edit::rewrite_block(&mut attempt, page, block, Some(text), &chosen).is_ok() {
            return Some(attempt);
        }
    }
    pdfcraft_fonts::document_thai_font()?;
    let mut attempt = doc.clone();
    let bundled = pdfcraft_edit::BlockStyle { resource: Some(pdfcraft_edit::text::BUILTIN_THAI_FONT.to_owned()), ..style.clone() };
    pdfcraft_edit::rewrite_block(&mut attempt, page, block, Some(text), &bundled).ok()?;
    Some(attempt)
}

/// Face `index` of a font file as a standalone program (a collection's face taken out of it).
pub fn standalone_face(bytes: &[u8], index: u32) -> Result<Vec<u8>, String> {
    pdfcraft_fonts::embedding::standalone_face(bytes, index)
}
