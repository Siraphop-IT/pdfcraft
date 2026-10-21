//! BMU PDF: Thai typed as Fill & Sign text or a text box comment is drawn in a face installed on
//! the machine (Tahoma first), not as `?`: flattened, the page's text reads back as the Thai typed,
//! and the comment can be moved afterwards. Skipped where no Thai face is installed.

use pdfcraft_engine::{Edit, NewAnnotation, Session, Shape, Style};

const THAI: &str = "ทดสอบภาษาไทย น้ำ 123";

fn add(shape: Shape) -> Option<(Session, pdfcraft_engine::DocId)> {
    if pdfcraft_fonts::system_face::for_text(THAI).is_none() {
        eprintln!("skipping: no installed Thai face here");
        return None;
    }
    let mut s = Session::new();
    let bytes = s.create_blank(400.0, 300.0, 1).unwrap();
    let id = s.open("blank.pdf", None, bytes, None).unwrap();
    let style = Style::default_for(&shape);
    s.apply(id, Edit::AddAnnotation(NewAnnotation { page: 0, shape, style, contents: THAI.into(), author: "BMU".into() })).unwrap();
    Some((s, id))
}

/// The first page's text as the viewer reads it (Find, copy).
fn viewer_text(s: &Session, id: pdfcraft_engine::DocId) -> String {
    let doc = s.get(id).unwrap();
    let request = pdfcraft_render::RenderRequest { page: 0, kind: pdfcraft_render::RequestKind::Text, scale: 1.0, ..Default::default() };
    let shot = pdfcraft_render::PageRenderer::new(doc.bytes.clone(), Default::default()).render(request);
    shot.text.map(|t| t.plain_text().trim().to_string()).unwrap_or_default()
}

#[test]
fn thai_fill_and_sign_text_flattens_to_the_thai_typed() {
    let Some((mut s, id)) = add(Shape::Typewriter { rect: [40.0, 200.0, 360.0, 230.0], font_size: 12.0 }) else { return };
    s.apply(id, Edit::FlattenFillSign).unwrap();
    assert_eq!(viewer_text(&s, id), THAI, "the page's text is the Thai typed, not ? marks");
}

#[test]
fn a_thai_text_box_is_drawn_and_can_be_moved() {
    let Some((mut s, id)) = add(Shape::TextBox { rect: [40.0, 150.0, 200.0, 230.0], font_size: 12.0 }) else { return };
    let saved = String::from_utf8_lossy(&s.save_bytes(id).unwrap()).into_owned();
    assert!(!saved.contains("/PCUndrawn"), "the text box is not marked as drawn with ?");
    assert!(saved.contains("/Type0"), "the Thai is drawn in an embedded face");
    // Moving it redraws it rather than refusing it as another program's drawing.
    s.apply(id, Edit::MoveAnnotation { page: 0, index: 0, dx: 10.0, dy: -10.0 }).unwrap();
}

/// Thai added in an installed face is text the viewer finds and copies: its marks, shaped into
/// their letter's cluster, add no empty `<>` entry that made the reader drop the whole map.
#[test]
fn thai_added_in_an_installed_face_is_text_the_viewer_reads() {
    let Some(face) = pdfcraft_fonts::system_face::for_text(THAI) else {
        eprintln!("skipping: no installed Thai face here");
        return;
    };
    let font = pdfcraft_engine::TextFont::new(face.bytes.to_vec()).unwrap();
    let mut s = Session::new();
    let bytes = s.create_blank(400.0, 300.0, 1).unwrap();
    let id = s.open("blank.pdf", None, bytes, None).unwrap();
    let text =
        pdfcraft_engine::AddedText { rect: [40.0, 200.0, 360.0, 260.0], text: THAI.into(), size: 18.0, font: Some(font), ..Default::default() };
    s.apply(id, Edit::AddText { page: 0, text }).unwrap();
    assert_eq!(viewer_text(&s, id), THAI);
}
