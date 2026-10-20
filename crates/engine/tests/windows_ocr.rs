//! Windows' own recogniser (Windows.Media.Ocr) reads a rendered page: the words come back with
//! boxes where they were drawn. Synthetic page; skipped where Windows has no English recogniser.
#![cfg(target_os = "windows")]

use pdfcraft_render::{PageRenderer, RenderRequest};

/// A 400 × 200 pt page with "INVOICE 2026" in 48 pt Helvetica, its baseline at y = 100.
fn page() -> Vec<u8> {
    let content = b"BT /F1 48 Tf 40 100 Td (INVOICE 2026) Tj ET";
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), String::from_utf8_lossy(content)),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
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

#[test]
fn windows_recognises_a_rendered_page() {
    let Some((tag, _)) = pdfcraft_ocr::system_languages().into_iter().find(|(t, _)| t.to_ascii_lowercase().starts_with("en")) else {
        eprintln!("skipping: Windows has no English recogniser here");
        return;
    };
    let ocr = pdfcraft_ocr::Ocr::for_language(&tag).expect("a listed language loads");
    let shot = PageRenderer::new(std::sync::Arc::new(page()), Default::default()).render(RenderRequest { page: 0, scale: 2.0, ..Default::default() });
    assert!(shot.error.is_none(), "{:?}", shot.error);
    let lines = ocr.recognize(&shot.rgba, shot.width, shot.height).expect("recognised");
    let words: Vec<_> = lines.iter().flat_map(|l| &l.words).collect();
    let text: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
    assert!(text.contains(&"INVOICE") && text.contains(&"2026"), "{text:?}");
    // The boxes are in the rendered image's pixels (2 per point): "INVOICE" starts at x = 40 pt
    // (80 px), is about 197 pt wide (ends near 474 px), and stands on the baseline 100 pt from
    // the top (200 px) with its cap height (34 pt) above.
    let invoice = words.iter().find(|w| w.text == "INVOICE").expect("found above");
    let [l, t, r, b] = invoice.rect;
    assert!(l > 60.0 && l < 100.0 && r > 420.0 && r < 520.0 && t > 100.0 && t < 160.0 && b > 180.0 && b < 220.0, "{:?}", invoice.rect);
}

#[test]
fn an_uninstalled_language_is_an_error_not_a_panic() {
    assert!(matches!(pdfcraft_ocr::Ocr::for_language("zz-ZZ"), Err(pdfcraft_ocr::OcrError::LanguageNotInstalled(_))));
}
