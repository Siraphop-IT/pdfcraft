//! Windows' own text recognition (`Windows.Media.Ocr`, Windows 10 and later). It reads every
//! language added in Settings ▸ Time & language ▸ Language & region (Thai among them) and needs no
//! model files of ours. The calls are WinRT through the `windows` crate, all safe Rust.

use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::core::HSTRING;

use crate::{Line, OcrError, Word};

/// A Windows recogniser for one language.
pub struct WindowsOcr {
    engine: OcrEngine,
}

impl WindowsOcr {
    /// The recogniser for `tag` (a BCP 47 tag such as `th`), if that language's recognition is
    /// installed in Windows.
    pub fn new(tag: &str) -> Result<WindowsOcr, OcrError> {
        let missing = || OcrError::LanguageNotInstalled(tag.to_string());
        let language = Language::CreateLanguage(&HSTRING::from(tag)).map_err(|_| missing())?;
        if !OcrEngine::IsLanguageSupported(&language).unwrap_or(false) {
            return Err(missing());
        }
        let engine = OcrEngine::TryCreateFromLanguage(&language).map_err(|_| missing())?;
        Ok(WindowsOcr { engine })
    }

    /// Recognise the text in an RGBA image, `width` × `height` pixels.
    pub fn recognize(&self, pixels: &[u8], width: u32, height: u32) -> Result<Vec<Line>, OcrError> {
        let err = |e: windows::core::Error| OcrError::Recognize(e.message());
        // u32 × u32 × 4 fits in u64, so these products can't overflow.
        let len = usize::try_from(u64::from(width) * u64::from(height) * 4).map_err(|_| OcrError::EmptyImage)?;
        if width == 0 || height == 0 || pixels.len() != len {
            return Err(OcrError::EmptyImage);
        }
        // The engine refuses larger images; a page rendered past its limit is read halved (or
        // less) and the boxes are scaled back.
        let max = OcrEngine::MaxImageDimension().map_err(err)?.max(1);
        let step = width.max(height).div_ceil(max).max(1);
        let (w, h) = (width.div_ceil(step), height.div_ceil(step));
        let mut bgra = Vec::with_capacity(usize::try_from(u64::from(w) * u64::from(h) * 4).map_err(|_| OcrError::EmptyImage)?);
        for y in 0..u64::from(h) {
            for x in 0..u64::from(w) {
                let at = (y * u64::from(step) * u64::from(width) + x * u64::from(step)) * 4;
                let at = usize::try_from(at).map_err(|_| OcrError::EmptyImage)?;
                let Some(&[r, g, b, _]) = pixels.get(at..at.saturating_add(4)) else { return Err(OcrError::EmptyImage) };
                bgra.extend_from_slice(&[b, g, r, 255]);
            }
        }
        let buffer = CryptographicBuffer::CreateFromByteArray(&bgra).map_err(err)?;
        let (wi, hi) = (i32::try_from(w).map_err(|_| OcrError::EmptyImage)?, i32::try_from(h).map_err(|_| OcrError::EmptyImage)?);
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, wi, hi).map_err(err)?;
        let result = self.engine.RecognizeAsync(&bitmap).map_err(err)?.join().map_err(err)?;
        let scale = step as f32;
        let mut lines = Vec::new();
        for line in result.Lines().map_err(err)? {
            let mut words = Vec::new();
            for word in line.Words().map_err(err)? {
                let text = word.Text().map_err(err)?.to_string_lossy();
                let r = word.BoundingRect().map_err(err)?;
                if text.trim().is_empty() || ![r.X, r.Y, r.Width, r.Height].iter().all(|v| v.is_finite()) {
                    continue;
                }
                words.push(Word { text, rect: [r.X * scale, r.Y * scale, (r.X + r.Width) * scale, (r.Y + r.Height) * scale] });
            }
            if !words.is_empty() {
                lines.push(Line { words });
            }
        }
        Ok(lines)
    }
}

/// The languages Windows can recognise on this machine: `(tag, name in that language)`.
pub fn languages() -> Vec<(String, String)> {
    let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else { return Vec::new() };
    list.into_iter()
        .filter_map(|l| {
            let tag = l.LanguageTag().ok()?.to_string_lossy();
            let name = l.NativeName().ok()?.to_string_lossy();
            (!tag.is_empty()).then_some((tag, name))
        })
        .collect()
}
