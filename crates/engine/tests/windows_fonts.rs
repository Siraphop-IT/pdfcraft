//! BMU PDF on Windows: the Thai supplemental fonts (Angsana New, Cordia New…), which Windows keeps
//! in .ttc collections, are listed as installed document fonts, and each listed face reads as a
//! standalone program that Add text can use. Skipped where they aren't installed.
#![cfg(target_os = "windows")]

use pdfcraft_engine::text_fonts::{discover, installed_directories};

#[test]
fn thai_collections_are_listed_and_usable() {
    let fonts = discover(&installed_directories());
    let names: Vec<&str> = fonts.iter().map(|f| f.family.as_str()).collect();
    eprintln!("Thai faces: {:?}", fonts.iter().filter(|f| f.thai).map(|f| (&f.face, f.path.display().to_string(), f.index)).collect::<Vec<_>>());
    let windir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_default().join("Fonts");
    let collections: Vec<_> = ["angsana.ttc", "cordia.ttc"].iter().filter(|f| windir.join(f).exists()).collect();
    if collections.is_empty() {
        eprintln!("skipping: the Thai supplemental fonts are not installed here");
        return;
    }
    for family in ["Angsana New", "Cordia New"] {
        assert!(names.contains(&family), "{family} is listed: {names:?}");
    }
    for font in fonts.iter().filter(|f| f.family == "Angsana New" || f.family == "Cordia New") {
        let bytes = font.read().unwrap_or_else(|e| panic!("{}: {e}", font.face));
        let text = pdfcraft_engine::TextFont::new(bytes).unwrap_or_else(|e| panic!("{}: {e}", font.face));
        assert_eq!(text.family, font.family);
    }
}
