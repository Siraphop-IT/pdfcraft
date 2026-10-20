//! Add text in a chosen font file (a Thai face installed on the machine): the text is shaped
//! with the face, embedded, reads back as the text typed, and the item keeps its font when it is
//! reopened and retyped. Uses Tahoma where it is installed (never committed); skipped elsewhere.

use pdfcraft_engine::{AddedContent, AddedText, Edit, Session, TextFont};

fn tahoma() -> Option<Vec<u8>> {
    ["/System/Library/Fonts/Supplemental/Tahoma.ttf", "C:\\Windows\\Fonts\\tahoma.ttf"].iter().find_map(|p| std::fs::read(p).ok())
}

fn blank() -> (Session, pdfcraft_engine::DocId) {
    let mut s = Session::new();
    let bytes = s.create_blank(400.0, 300.0, 1).unwrap();
    let id = s.open("blank.pdf", None, bytes, None).unwrap();
    (s, id)
}

#[test]
fn thai_added_in_an_installed_face_reads_back_and_keeps_its_font() {
    let Some(bytes) = tahoma() else {
        eprintln!("skipping: Tahoma is not installed here");
        return;
    };
    let font = TextFont::new(bytes).expect("Tahoma can be embedded");
    assert_eq!(font.family, "Tahoma");
    let thai = "ใบกำกับภาษี เลขที่ 001";
    let (mut s, id) = blank();
    let text = AddedText { rect: [40.0, 200.0, 360.0, 260.0], text: thai.into(), size: 18.0, font: Some(font.clone()), ..Default::default() };
    s.apply(id, Edit::AddText { page: 0, text }).unwrap();
    let read: Vec<String> = s.get(id).unwrap().text_blocks(0).into_iter().map(|b| b.text).collect();
    assert_eq!(read.join(" "), thai, "the page's text is the Thai typed, not ? marks");

    // Saved and reopened, the item is still in Tahoma, and retyping keeps it.
    let saved = s.save_bytes(id).unwrap();
    let mut again = Session::new();
    let id = again.open("thai.pdf", None, saved, None).unwrap();
    let added = again.get(id).unwrap().added().to_vec();
    let Some(AddedContent::Text(item)) = added.first().map(|a| a.content.clone()) else { panic!("the added text is listed: {added:?}") };
    assert_eq!(item.font.as_ref().map(|f| f.family.as_str()), Some("Tahoma"));
    let retyped = AddedText { text: "สวัสดี".into(), ..item };
    again.apply(id, Edit::UpdateContent { page: 0, index: 0, content: AddedContent::Text(retyped) }).unwrap();
    let read: Vec<String> = again.get(id).unwrap().text_blocks(0).into_iter().map(|b| b.text).collect();
    assert_eq!(read.join(" "), "สวัสดี");
}

#[test]
fn a_character_the_face_lacks_is_refused_not_drawn_as_a_box() {
    let Some(bytes) = tahoma() else {
        eprintln!("skipping: Tahoma is not installed here");
        return;
    };
    let font = TextFont::new(bytes).unwrap();
    let (mut s, id) = blank();
    let text = AddedText { rect: [40.0, 200.0, 360.0, 260.0], text: "ก 你".into(), font: Some(font), ..Default::default() };
    let e = s.apply(id, Edit::AddText { page: 0, text }).unwrap_err().to_string();
    assert!(e.contains("Tahoma has no glyph for \"你\""), "{e}");
}

#[test]
fn a_file_that_is_not_a_font_is_an_error_not_a_panic() {
    assert!(TextFont::new(b"not a font".to_vec()).is_err());
    assert!(TextFont::new(Vec::new()).is_err());
}
