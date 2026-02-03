use serde::{Deserialize, Serialize};

/// Client configuration that persists between sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub client_id: String,
    pub password: Option<String>,
    pub server_url: String,
    pub connection_history: Vec<ConnectionHistoryEntry>,
    pub reconnection_config: ReconnectionConfig,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            client_id: uuid::Uuid::new_v4().to_string(),
            password: None,
            server_url: "http://localhost:5000".to_string(),
            connection_history: Vec::new(),
            reconnection_config: ReconnectionConfig::default(),
        }
    }
}

/// Reconnection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectionConfig {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for ReconnectionConfig {
    fn default() -> Self {
        Self {
            max_retries: 10,
            base_delay_ms: 2000,
            max_delay_ms: 30000,
        }
    }
}

/// Entry in connection history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionHistoryEntry {
    pub client_id: String,
    pub last_connected: String,
    pub alias: Option<String>,
}

/// Frame data for screen streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameData {
    #[serde(rename = "imageData")]
    pub image_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub timestamp: i64,
}

/// Input event data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputData {
    #[serde(rename = "type")]
    pub input_type: InputType,
    pub x: i32,
    pub y: i32,
    pub button: i32,
    #[serde(rename = "keyCode")]
    pub key_code: i32,
    #[serde(rename = "keyChar")]
    pub key_char: Option<String>,
    #[serde(rename = "isKeyDown")]
    pub is_key_down: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum InputType {
    MouseMove = 0,
    MouseDown = 1,
    MouseUp = 2,
    MouseScroll = 3,
    KeyDown = 4,
    KeyUp = 5,
}

/// Connection state
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    InSession,
}

/// Application state
#[derive(Debug, Clone)]
pub struct AppState {
    pub connection_state: ConnectionState,
    pub current_peer: Option<String>,
    pub error_message: Option<String>,
    pub pending_request: Option<String>,
    pub last_connection_time: Option<i64>,
    pub reconnection_attempt: u32,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            connection_state: ConnectionState::Disconnected,
            current_peer: None,
            error_message: None,
            pending_request: None,
            last_connection_time: None,
            reconnection_attempt: 0,
        }
    }
}

/// File transfer metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferData {
    #[serde(rename = "transferId")]
    pub transfer_id: String,
    #[serde(rename = "filename")]
    pub filename: String,
    #[serde(rename = "fileSize")]
    pub file_size: i64,
    #[serde(rename = "totalChunks")]
    pub total_chunks: i32,
}

/// File chunk data for transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    #[serde(rename = "transferId")]
    pub transfer_id: String,
    #[serde(rename = "chunkIndex")]
    pub chunk_index: i32,
    #[serde(rename = "data")]
    pub data: Vec<u8>,
    #[serde(rename = "checksum")]
    pub checksum: String,
}
