use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{sleep, interval};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::models::{FrameData, InputData};

/// Messages from server to client
#[derive(Debug, Clone)]
pub enum ServerMessage {
    Registered(String),
    ConnectionRequest(String),
    ConnectionAccepted(String),
    ConnectionRejected,
    ConnectionEstablished(String),
    PeerDisconnected,
    ReceiveFrame(FrameData),
    ReceiveInput(InputData),
    Error(String),
}

/// Messages from client to server
#[derive(Debug, Clone)]
pub enum ClientMessage {
    Register(String, Option<String>),
    RequestConnection(String, Option<String>),
    AcceptConnection(String),
    RejectConnection(String),
    SendFrame(FrameData),
    SendInput(InputData),
    Heartbeat,
    DisconnectSession,
}

/// Configuration for reconnection behavior
#[derive(Debug, Clone)]
pub struct ReconnectionConfig {
    /// Maximum number of reconnection attempts (0 = no retries)
    pub max_attempts: u32,
    /// Initial delay between reconnection attempts in milliseconds
    pub initial_delay_ms: u64,
    /// Maximum delay between reconnection attempts in milliseconds
    pub max_delay_ms: u64,
    /// Multiplier for exponential backoff (e.g., 2.0 doubles the delay each attempt)
    pub backoff_multiplier: f64,
}

impl Default for ReconnectionConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2.0,
        }
    }
}

/// Connection state
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Reconnecting,
    Failed,
}

/// Connection health monitoring
#[derive(Debug, Clone)]
pub struct ConnectionHealth {
    /// Current connection state
    pub state: ConnectionState,
    /// Last successful heartbeat timestamp
    pub last_heartbeat: Option<Instant>,
    /// Number of consecutive failed heartbeats
    pub failed_heartbeats: u32,
    /// Total reconnection attempts
    pub reconnection_attempts: u32,
}

impl Default for ConnectionHealth {
    fn default() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            last_heartbeat: None,
            failed_heartbeats: 0,
            reconnection_attempts: 0,
        }
    }
}

impl ConnectionHealth {
    /// Check if connection is healthy
    pub fn is_healthy(&self) -> bool {
        self.state == ConnectionState::Connected && self.failed_heartbeats < 3
    }

    /// Update heartbeat success
    pub fn heartbeat_success(&mut self) {
        self.last_heartbeat = Some(Instant::now());
        self.failed_heartbeats = 0;
        self.state = ConnectionState::Connected;
    }

    /// Update heartbeat failure
    pub fn heartbeat_failure(&mut self) {
        self.failed_heartbeats += 1;
    }

    /// Mark connection as disconnected
    pub fn mark_disconnected(&mut self) {
        self.state = ConnectionState::Disconnected;
    }

    /// Mark connection as reconnecting
    pub fn mark_reconnecting(&mut self) {
        self.state = ConnectionState::Reconnecting;
        self.reconnection_attempts += 1;
    }

    /// Mark connection as failed
    pub fn mark_failed(&mut self) {
        self.state = ConnectionState::Failed;
    }
}

/// SignalR-like connection to the relay server
pub struct RelayConnection {
    sender: mpsc::Sender<ClientMessage>,
    receiver: Arc<Mutex<mpsc::Receiver<ServerMessage>>>,
    health: Arc<RwLock<ConnectionHealth>>,
}

impl RelayConnection {
    /// Connect to the relay server
    pub async fn connect(server_url: &str) -> Result<(Self, mpsc::Receiver<ServerMessage>)> {
        let ws_url = format!("{}/hub", server_url.replace("http", "ws"));
        let (ws_stream, _) = connect_async(&ws_url).await?;
        let (mut write, mut read) = ws_stream.split();

        let (client_tx, mut client_rx) = mpsc::channel::<ClientMessage>(100);
        let (server_tx, server_rx) = mpsc::channel::<ServerMessage>(100);
        let server_rx_arc = Arc::new(Mutex::new(server_rx));
        let health = Arc::new(RwLock::new(ConnectionHealth::default()));

        // Spawn task to handle outgoing messages
        let server_tx_clone = server_tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = client_rx.recv().await {
                let json_msg = match msg {
                    ClientMessage::Register(id, password) => {
                        json!({
                            "type": 1,
                            "target": "Register",
                            "arguments": [id, password]
                        })
                    }
                    ClientMessage::RequestConnection(target_id, password) => {
                        json!({
                            "type": 1,
                            "target": "RequestConnection",
                            "arguments": [target_id, password]
                        })
                    }
                    ClientMessage::AcceptConnection(requester_id) => {
                        json!({
                            "type": 1,
                            "target": "AcceptConnection",
                            "arguments": [requester_id]
                        })
                    }
                    ClientMessage::RejectConnection(requester_id) => {
                        json!({
                            "type": 1,
                            "target": "RejectConnection",
                            "arguments": [requester_id]
                        })
                    }
                    ClientMessage::SendFrame(frame) => {
                        json!({
                            "type": 1,
                            "target": "SendFrame",
                            "arguments": [frame]
                        })
                    }
                    ClientMessage::SendInput(input) => {
                        json!({
                            "type": 1,
                            "target": "SendInput",
                            "arguments": [input]
                        })
                    }
                    ClientMessage::Heartbeat => {
                        json!({
                            "type": 1,
                            "target": "Heartbeat",
                            "arguments": []
                        })
                    }
                    ClientMessage::DisconnectSession => {
                        json!({
                            "type": 1,
                            "target": "DisconnectSession",
                            "arguments": []
                        })
                    }
                };

                // SignalR uses \x1e as message terminator
                let msg_str = format!("{}\x1e", json_msg);
                if write.send(Message::Text(msg_str)).await.is_err() {
                    break;
                }
            }
        });

        // Spawn task to handle incoming messages
        tokio::spawn(async move {
            while let Some(Ok(msg)) = read.next().await {
                if let Message::Text(text) = msg {
                    // SignalR messages are terminated with \x1e
                    for part in text.split('\x1e').filter(|s| !s.is_empty()) {
                        if let Ok(json) = serde_json::from_str::<Value>(part) {
                            if let Some(target) = json.get("target").and_then(|t| t.as_str()) {
                                let args = json.get("arguments").and_then(|a| a.as_array());
                                
                                let server_msg = match target {
                                    "Registered" => {
                                        args.and_then(|a| a.first())
                                            .and_then(|v| v.as_str())
                                            .map(|id| ServerMessage::Registered(id.to_string()))
                                    }
                                    "ConnectionRequest" => {
                                        args.and_then(|a| a.first())
                                            .and_then(|v| v.as_str())
                                            .map(|id| ServerMessage::ConnectionRequest(id.to_string()))
                                    }
                                    "ConnectionAccepted" => {
                                        args.and_then(|a| a.first())
                                            .and_then(|v| v.as_str())
                                            .map(|id| ServerMessage::ConnectionAccepted(id.to_string()))
                                    }
                                    "ConnectionRejected" => Some(ServerMessage::ConnectionRejected),
                                    "ConnectionEstablished" => {
                                        args.and_then(|a| a.first())
                                            .and_then(|v| v.as_str())
                                            .map(|id| ServerMessage::ConnectionEstablished(id.to_string()))
                                    }
                                    "PeerDisconnected" => Some(ServerMessage::PeerDisconnected),
                                    "ReceiveFrame" => {
                                        args.and_then(|a| a.first())
                                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                                            .map(ServerMessage::ReceiveFrame)
                                    }
                                    "ReceiveInput" => {
                                        args.and_then(|a| a.first())
                                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                                            .map(ServerMessage::ReceiveInput)
                                    }
                                    _ => None,
                                };

                                if let Some(msg) = server_msg {
                                    if server_tx_clone.send(msg).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        let (out_server_tx, out_server_rx) = mpsc::channel(100);

        // Forward messages and track connection health
        let server_rx_clone = server_rx_arc.clone();
        let health_clone = health.clone();
        tokio::spawn(async move {
            let mut rx = server_rx_clone.lock().await;
            while let Some(msg) = rx.recv().await {
                // Update health on successful message
                health_clone.write().await.heartbeat_success();

                if out_server_tx.send(msg).await.is_err() {
                    health_clone.write().await.mark_disconnected();
                    break;
                }
            }
            health_clone.write().await.mark_disconnected();
        });

        // Mark connection as established
        health.write().await.state = ConnectionState::Connected;
        health.write().await.heartbeat_success();

        Ok((
            Self {
                sender: client_tx,
                receiver: server_rx_arc,
                health: health.clone(),
            },
            out_server_rx,
        ))
    }

    /// Connect to the relay server with automatic reconnection and exponential backoff
    pub async fn connect_with_retry(
        server_url: &str,
        config: ReconnectionConfig,
    ) -> Result<(Self, mpsc::Receiver<ServerMessage>)> {
        let mut attempt = 0;
        let mut delay_ms = config.initial_delay_ms;

        loop {
            match Self::connect(server_url).await {
                Ok(result) => {
                    if attempt > 0 {
                        log::info!("Successfully reconnected to server after {} attempts", attempt);
                    }
                    return Ok(result);
                }
                Err(e) => {
                    attempt += 1;

                    if attempt > config.max_attempts {
                        log::error!(
                            "Failed to connect after {} attempts: {}",
                            config.max_attempts,
                            e
                        );
                        return Err(e);
                    }

                    log::warn!(
                        "Connection attempt {} failed: {}. Retrying in {}ms...",
                        attempt,
                        e,
                        delay_ms
                    );

                    sleep(Duration::from_millis(delay_ms)).await;

                    // Calculate next delay with exponential backoff
                    delay_ms = ((delay_ms as f64) * config.backoff_multiplier) as u64;
                    delay_ms = delay_ms.min(config.max_delay_ms);
                }
            }
        }
    }

    /// Send a message to the server
    pub async fn send(&self, msg: ClientMessage) -> Result<()> {
        self.sender.send(msg).await?;
        Ok(())
    }

    /// Get current connection health
    pub async fn get_health(&self) -> ConnectionHealth {
        self.health.read().await.clone()
    }

    /// Check if connection is healthy
    pub async fn is_healthy(&self) -> bool {
        self.health.read().await.is_healthy()
    }

    /// Start heartbeat monitoring task
    /// Sends periodic heartbeats and monitors connection health
    pub fn start_heartbeat_monitor(&self, interval_ms: u64) {
        let sender = self.sender.clone();
        let health = self.health.clone();

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(interval_ms));

            loop {
                ticker.tick().await;

                // Check if we should still be monitoring
                let current_health = health.read().await.clone();
                if current_health.state == ConnectionState::Failed {
                    log::info!("Stopping heartbeat monitor - connection failed");
                    break;
                }

                // Send heartbeat
                if let Err(e) = sender.send(ClientMessage::Heartbeat).await {
                    log::warn!("Failed to send heartbeat: {}", e);
                    health.write().await.heartbeat_failure();

                    // Check if we've exceeded failure threshold
                    let failed_count = health.read().await.failed_heartbeats;
                    if failed_count >= 3 {
                        log::error!("Connection unhealthy - {} consecutive heartbeat failures", failed_count);
                        health.write().await.mark_disconnected();
                    }
                } else {
                    // Heartbeat sent successfully
                    log::trace!("Heartbeat sent successfully");
                }

                // Check for stale connection (no response for 30 seconds)
                if let Some(last_hb) = health.read().await.last_heartbeat {
                    if last_hb.elapsed() > Duration::from_secs(30) {
                        log::warn!("Connection appears stale - no heartbeat response in 30 seconds");
                        health.write().await.mark_disconnected();
                    }
                }
            }
        });
    }
}
