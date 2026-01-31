use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
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

/// SignalR-like connection to the relay server
pub struct RelayConnection {
    sender: mpsc::Sender<ClientMessage>,
    receiver: Arc<Mutex<mpsc::Receiver<ServerMessage>>>,
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
        
        // Forward messages
        let server_rx_clone = server_rx_arc.clone();
        tokio::spawn(async move {
            let mut rx = server_rx_clone.lock().await;
            while let Some(msg) = rx.recv().await {
                if out_server_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        Ok((
            Self {
                sender: client_tx,
                receiver: server_rx_arc,
            },
            out_server_rx,
        ))
    }

    /// Send a message to the server
    pub async fn send(&self, msg: ClientMessage) -> Result<()> {
        self.sender.send(msg).await?;
        Ok(())
    }
}
