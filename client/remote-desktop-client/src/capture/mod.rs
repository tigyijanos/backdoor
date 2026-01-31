use anyhow::Result;
use scrap::{Capturer, Display};
use std::io::ErrorKind;
use std::time::Instant;

use crate::models::FrameData;

/// Screen capture functionality
pub struct ScreenCapture {
    capturer: Capturer,
    width: u32,
    height: u32,
}

impl ScreenCapture {
    /// Create a new screen capturer for the primary display
    pub fn new() -> Result<Self> {
        let display = Display::primary()?;
        let width = display.width() as u32;
        let height = display.height() as u32;
        let capturer = Capturer::new(display)?;

        Ok(Self {
            capturer,
            width,
            height,
        })
    }

    /// Capture the current screen and return as JPEG-encoded frame data
    pub fn capture(&mut self) -> Result<Option<FrameData>> {
        match self.capturer.frame() {
            Ok(frame) => {
                // Convert BGRA to RGB
                let mut rgb_data = Vec::with_capacity((self.width * self.height * 3) as usize);
                for chunk in frame.chunks(4) {
                    rgb_data.push(chunk[2]); // R
                    rgb_data.push(chunk[1]); // G
                    rgb_data.push(chunk[0]); // B
                }

                // Encode as JPEG for compression
                let image_data = encode_jpeg(&rgb_data, self.width, self.height)?;

                Ok(Some(FrameData {
                    image_data,
                    width: self.width,
                    height: self.height,
                    format: "jpeg".to_string(),
                    timestamp: Instant::now().elapsed().as_millis() as i64,
                }))
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Encode RGB data as JPEG
fn encode_jpeg(rgb_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    use image::{ImageBuffer, Rgb};
    use std::io::Cursor;

    let img: ImageBuffer<Rgb<u8>, _> =
        ImageBuffer::from_raw(width, height, rgb_data.to_vec())
            .ok_or_else(|| anyhow::anyhow!("Failed to create image buffer"))?;

    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, image::ImageOutputFormat::Jpeg(80))?;

    Ok(buffer.into_inner())
}
