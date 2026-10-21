//! BMU PDF: rewriting a Thai paragraph whose font isn't embedded (or is a subset that lacks the
//! new characters) needs no font chosen by hand: the same family installed on the machine draws
//! it (Tahoma here), else the bundled Thai face. Synthetic PDF; skipped where Tahoma is missing.

use pdfcraft_engine::{BlockStyle, Edit, Session};

/// A page with "สวัสดี" in a Type 0 font named `base_font` with no font program, its codes mapped
/// to Thai by a ToUnicode CMap.
fn thai_page(base_font: &str) -> Vec<u8> {
    let thai: Vec<char> = "สวัสดี".chars().collect();
    let codes: String = (1..=thai.len()).map(|i| format!("{i:04X}")).collect();
    let content = format!("BT /F1 20 Tf 50 700 Td <{codes}> Tj ET");
    let bfchar: String = thai.iter().enumerate().map(|(i, c)| format!("<{:04X}> <{:04X}>\n", i + 1, u32::from(*c))).collect();
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CMapName /T def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange {} beginbfchar\n{bfchar}endbfchar endcmap CMapName currentdict /CMap defineresource pop end end",
        thai.len()
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /Type0 /BaseFont /{base_font} /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>"),
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{base_font} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 500 >>"
        ),
        format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len()),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).into_bytes());
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
    for o in offsets {
        pdf.extend(format!("{o:010} 00000 n \n").into_bytes());
    }
    pdf.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).into_bytes());
    pdf
}

fn edit(base_font: &str, text: &str) -> Result<String, String> {
    let mut s = Session::new();
    let id = s.open("thai.pdf", None, std::sync::Arc::new(thai_page(base_font)), None).map_err(|e| e.to_string())?;
    let blocks = s.get(id).unwrap().text_blocks(0);
    assert_eq!(blocks.first().map(|b| b.text.as_str()), Some("สวัสดี"), "the synthetic page reads as Thai");
    s.apply(id, Edit::EditTextBlock { page: 0, block: 0, text: text.into(), style: BlockStyle::default() }).map_err(|e| e.to_string())?;
    let read: Vec<String> = s.get(id).unwrap().text_blocks(0).into_iter().map(|b| b.text).collect();
    Ok(read.join(" "))
}

#[test]
fn a_thai_paragraph_is_rewritten_in_its_installed_family() {
    if !["/System/Library/Fonts/Supplemental/Tahoma.ttf", "C:\\Windows\\Fonts\\tahoma.ttf"].iter().any(|p| std::path::Path::new(p).exists()) {
        eprintln!("skipping: Tahoma is not installed here");
        return;
    }
    assert_eq!(edit("Tahoma", "สวัสดีครับ เอ็ม").unwrap(), "สวัสดีครับ เอ็ม");
}

#[test]
fn a_thai_paragraph_in_a_missing_family_takes_the_bundled_face_or_says_why() {
    match (edit("NoSuchThaiFace", "สวัสดีครับ"), pdfcraft_fonts::document_thai_font()) {
        (Ok(text), Some(_)) => assert_eq!(text, "สวัสดีครับ"),
        (Err(e), None) => assert!(!e.is_empty()),
        (r, bundled) => panic!("{r:?} with bundled Thai face {}", bundled.is_some()),
    }
}
