use crate::r6::{upscale_bgra, Line, Word};
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

const SCALE: u32 = 2;

pub struct Reader {
    engine: OcrEngine,
}

impl Reader {
    pub fn new() -> Option<Self> {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .ok()
            .or_else(|| {
                Language::CreateLanguage(&HSTRING::from("en-US"))
                    .ok()
                    .and_then(|l| OcrEngine::TryCreateFromLanguage(&l).ok())
            })?;
        Some(Self { engine })
    }

    pub fn read(&self, gray: &[u8], width: u32, height: u32) -> Option<Vec<Line>> {
        if width == 0 || height == 0 || gray.len() < (width * height) as usize {
            return None;
        }
        let (bgra, w, h) = upscale_bgra(gray, width, height, SCALE);
        let writer = DataWriter::new().ok()?;
        writer.WriteBytes(&bgra).ok()?;
        let buffer = writer.DetachBuffer().ok()?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            w as i32,
            h as i32,
        )
        .ok()?;
        let result = self.engine.RecognizeAsync(&bitmap).ok()?.join().ok()?;
        let mut lines = Vec::new();
        for line in result.Lines().ok()? {
            let mut words = Vec::new();
            for word in line.Words().ok()? {
                let rect = word.BoundingRect().ok()?;
                let k = SCALE as f32;
                words.push(Word {
                    text: word.Text().ok()?.to_string(),
                    x: rect.X / k,
                    y: rect.Y / k,
                    w: rect.Width / k,
                    h: rect.Height / k,
                });
            }
            lines.push(Line { words });
        }
        Some(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r6::{read_feed, Reading, REGION};
    use std::path::Path;

    fn bgra_of(path: &Path) -> (Vec<u8>, u32, u32) {
        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        let channels = match info.color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            other => panic!("unsupported picture type {other:?}"),
        };
        let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
        for px in buf[..info.buffer_size()].chunks_exact(channels) {
            out.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
        (out, info.width, info.height)
    }

    fn reading(reader: &Reader, path: &Path) -> Reading {
        let (bgra, w, h) = bgra_of(path);
        let (cw, ch, gray) =
            sc_core::region::crop_gray(&bgra, (w * 4) as usize, w, h, REGION).unwrap();
        let lines = reader.read(&gray, cw, ch).unwrap();
        for line in &lines {
            eprintln!(
                "  {:?}",
                line.words
                    .iter()
                    .map(|w| (&w.text, w.x))
                    .collect::<Vec<_>>()
            );
        }
        read_feed(&lines, cw as f32, None)
    }

    #[test]
    #[ignore = "needs SC_R6_FRAMES, a folder of real full resolution screenshots"]
    fn real_screenshots_read_as_kills_and_deaths() {
        let dir = std::env::var("SC_R6_FRAMES").expect("set SC_R6_FRAMES");
        let reader = Reader::new().expect("no text recognition on this computer");
        let cases = [
            (
                "kill-1",
                Reading {
                    kills: 1,
                    team_kills: 0,
                    deaths: 0,
                },
            ),
            (
                "trade",
                Reading {
                    kills: 1,
                    team_kills: 0,
                    deaths: 1,
                },
            ),
            (
                "death-1",
                Reading {
                    kills: 0,
                    team_kills: 0,
                    deaths: 1,
                },
            ),
            (
                "death-2",
                Reading {
                    kills: 0,
                    team_kills: 0,
                    deaths: 1,
                },
            ),
            (
                "menu",
                Reading {
                    kills: 0,
                    team_kills: 0,
                    deaths: 0,
                },
            ),
        ];
        for (name, expected) in cases {
            let path = Path::new(&dir).join(format!("{name}.png"));
            eprintln!("{name}:");
            assert_eq!(reading(&reader, &path), expected, "{name}");
        }
    }

    #[test]
    fn the_reader_starts_and_reads_a_blank_picture_as_nothing() {
        let reader = Reader::new().expect("no text recognition on this computer");
        let blank = vec![30u8; 300 * 120];
        assert_eq!(reader.read(&blank, 300, 120).unwrap().len(), 0);
        assert!(reader.read(&blank, 0, 0).is_none());
        assert!(reader.read(&blank[..10], 300, 120).is_none());
    }
}
