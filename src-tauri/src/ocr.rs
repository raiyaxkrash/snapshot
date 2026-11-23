//! Оффлайн-распознавание текста (OCR) через встроенный WinRT `Windows.Media.Ocr`.
//! Поддерживает русский, английский и все языки, установленные в профиле Windows.

use windows::Graphics::Imaging::BitmapDecoder;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

use crate::capture::ComGuard;

pub fn recognize_png(png: &[u8]) -> Result<String, &'static str> {
    let _com = ComGuard::init();
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| "no_ocr_lang")?;
    let stream = InMemoryRandomAccessStream::new().map_err(|_| "stream")?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(|_| "stream")?;
    writer.WriteBytes(png).map_err(|_| "stream")?;
    writer.StoreAsync().map_err(|_| "stream")?.join().map_err(|_| "stream")?;
    writer.FlushAsync().map_err(|_| "stream")?.join().map_err(|_| "stream")?;
    let _ = writer.DetachStream();
    stream.Seek(0).map_err(|_| "stream")?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|_| "decode")?
        .join()
        .map_err(|_| "decode")?;
    let bmp = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|_| "decode")?
        .join()
        .map_err(|_| "decode")?;
    let res = engine
        .RecognizeAsync(&bmp)
        .map_err(|_| "ocr")?
        .join()
        .map_err(|_| "ocr")?;
    let lines = res.Lines().map_err(|_| "ocr")?;
    let mut out = Vec::new();
    for line in lines {
        if let Ok(t) = line.Text() {
            let s = t.to_string();
            if !s.trim().is_empty() {
                out.push(s);
            }
        }
    }
    Ok(out.join("\n"))
}
