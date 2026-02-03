use anyhow::Result;
use arboard::{Clipboard as ArboardClipboard, ImageData};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{ClipboardData, ClipboardType};

/// Clipboard monitoring and synchronization functionality
pub struct ClipboardMonitor {
    clipboard: ArboardClipboard,
    last_text: Option<String>,
    last_image_hash: Option<u64>,
}

impl ClipboardMonitor {
    /// Create a new clipboard monitor
    pub fn new() -> Result<Self> {
        let clipboard = ArboardClipboard::new()?;

        Ok(Self {
            clipboard,
            last_text: None,
            last_image_hash: None,
        })
    }

    /// Get the current clipboard content
    pub fn get_clipboard_content(&mut self) -> Result<Option<ClipboardData>> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        // Try to get text first
        if let Ok(text) = self.clipboard.get_text() {
            return Ok(Some(ClipboardData {
                clipboard_type: ClipboardType::Text,
                text_data: text,
                image_data: Vec::new(),
                timestamp,
            }));
        }

        // Try to get image if no text
        if let Ok(image) = self.clipboard.get_image() {
            let image_data = encode_image(&image);
            return Ok(Some(ClipboardData {
                clipboard_type: ClipboardType::Image,
                text_data: String::new(),
                image_data,
                timestamp,
            }));
        }

        Ok(None)
    }

    /// Set the clipboard content
    pub fn set_clipboard_content(&mut self, data: &ClipboardData) -> Result<()> {
        match data.clipboard_type {
            ClipboardType::Text => {
                self.clipboard.set_text(&data.text_data)?;
                self.last_text = Some(data.text_data.clone());
            }
            ClipboardType::Image => {
                let image = decode_image(&data.image_data)?;
                self.clipboard.set_image(image)?;
                self.last_image_hash = Some(hash_bytes(&data.image_data));
            }
        }
        Ok(())
    }

    /// Check if the clipboard content has changed since last check
    pub fn has_changed(&mut self) -> Result<bool> {
        // Check for text changes
        if let Ok(text) = self.clipboard.get_text() {
            if self.last_text.as_ref() != Some(&text) {
                return Ok(true);
            }
        }

        // Check for image changes
        if let Ok(image) = self.clipboard.get_image() {
            let image_data = encode_image(&image);
            let hash = hash_bytes(&image_data);
            if self.last_image_hash != Some(hash) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Update the internal state with current clipboard content
    pub fn update_state(&mut self) -> Result<()> {
        if let Ok(text) = self.clipboard.get_text() {
            self.last_text = Some(text);
        }

        if let Ok(image) = self.clipboard.get_image() {
            let image_data = encode_image(&image);
            self.last_image_hash = Some(hash_bytes(&image_data));
        }

        Ok(())
    }
}

/// Encode arboard ImageData to PNG bytes
fn encode_image(image: &ImageData) -> Vec<u8> {
    use image::{ImageBuffer, Rgba};
    use std::io::Cursor;

    let img: ImageBuffer<Rgba<u8>, _> =
        ImageBuffer::from_raw(image.width as u32, image.height as u32, image.bytes.to_vec())
            .expect("Failed to create image buffer");

    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, image::ImageOutputFormat::Png)
        .expect("Failed to encode PNG");

    buffer.into_inner()
}

/// Decode PNG bytes to arboard ImageData
fn decode_image(data: &[u8]) -> Result<ImageData<'static>> {
    use image::ImageFormat;
    use std::io::Cursor;

    let img = image::load(Cursor::new(data), ImageFormat::Png)?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    Ok(ImageData {
        width: width as usize,
        height: height as usize,
        bytes: rgba.into_raw().into(),
    })
}

/// Simple hash function for byte arrays
fn hash_bytes(data: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}
