use anyhow::Result;
use enigo::{Enigo, Key, KeyboardControllable, MouseButton, MouseControllable};

use crate::models::{InputData, InputType};

/// Input handler for simulating keyboard and mouse input
pub struct InputHandler {
    enigo: Enigo,
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            enigo: Enigo::new(),
        }
    }

    /// Process input data and simulate the corresponding input
    pub fn process_input(&mut self, input: &InputData) -> Result<()> {
        match input.input_type {
            InputType::MouseMove => {
                self.enigo.mouse_move_to(input.x, input.y);
            }
            InputType::MouseDown => {
                let button = match input.button {
                    0 => MouseButton::Left,
                    1 => MouseButton::Middle,
                    2 => MouseButton::Right,
                    _ => MouseButton::Left,
                };
                self.enigo.mouse_down(button);
            }
            InputType::MouseUp => {
                let button = match input.button {
                    0 => MouseButton::Left,
                    1 => MouseButton::Middle,
                    2 => MouseButton::Right,
                    _ => MouseButton::Left,
                };
                self.enigo.mouse_up(button);
            }
            InputType::MouseScroll => {
                self.enigo.mouse_scroll_y(input.y);
            }
            InputType::KeyDown => {
                if let Some(key) = self.map_key_code(input.key_code) {
                    self.enigo.key_down(key);
                }
            }
            InputType::KeyUp => {
                if let Some(key) = self.map_key_code(input.key_code) {
                    self.enigo.key_up(key);
                }
            }
        }
        Ok(())
    }

    fn map_key_code(&self, key_code: i32) -> Option<Key> {
        // Map common key codes to enigo keys
        match key_code {
            8 => Some(Key::Backspace),
            9 => Some(Key::Tab),
            13 => Some(Key::Return),
            16 => Some(Key::Shift),
            17 => Some(Key::Control),
            18 => Some(Key::Alt),
            20 => Some(Key::CapsLock),
            27 => Some(Key::Escape),
            32 => Some(Key::Space),
            37 => Some(Key::LeftArrow),
            38 => Some(Key::UpArrow),
            39 => Some(Key::RightArrow),
            40 => Some(Key::DownArrow),
            46 => Some(Key::Delete),
            // Letters A-Z (65-90)
            65..=90 => Some(Key::Layout(char::from_u32(key_code as u32 + 32)?)),
            // Numbers 0-9 (48-57)
            48..=57 => Some(Key::Layout(char::from_u32(key_code as u32)?)),
            // Function keys F1-F12 (112-123)
            112 => Some(Key::F1),
            113 => Some(Key::F2),
            114 => Some(Key::F3),
            115 => Some(Key::F4),
            116 => Some(Key::F5),
            117 => Some(Key::F6),
            118 => Some(Key::F7),
            119 => Some(Key::F8),
            120 => Some(Key::F9),
            121 => Some(Key::F10),
            122 => Some(Key::F11),
            123 => Some(Key::F12),
            _ => None,
        }
    }
}

impl Default for InputHandler {
    fn default() -> Self {
        Self::new()
    }
}
