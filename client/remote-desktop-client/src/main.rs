mod capture;
mod file_transfer;
mod input;
mod models;
mod network;

use anyhow::Result;
use eframe::egui;
use file_transfer::FileTransferManager;
use models::{AppState, ClientConfig, ConnectionHistoryEntry, ConnectionState, InputData, InputType};
use network::{ClientMessage, RelayConnection, ReconnectionConfig as NetworkReconnectionConfig, ServerMessage};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

fn main() -> Result<()> {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 500.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Remote Desktop Client",
        options,
        Box::new(|_cc| Box::new(RemoteDesktopApp::new())),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run app: {}", e))?;

    Ok(())
}

struct RemoteDesktopApp {
    config: ClientConfig,
    state: AppState,
    target_id_input: String,
    password_input: String,
    new_password_input: String,
    server_url_input: String,
    
    // Runtime
    runtime: tokio::runtime::Runtime,
    connection: Option<Arc<Mutex<RelayConnection>>>,
    server_rx: Option<mpsc::Receiver<ServerMessage>>,
    
    // Remote view
    remote_frame: Option<egui::TextureHandle>,
    frame_data: Option<models::FrameData>,

    // File transfer
    file_transfer_manager: Option<FileTransferManager>,

    // Settings panel
    show_settings: bool,

    // Connection details panel
    show_connection_details: bool,

    // File transfer panel
    show_file_transfer: bool,
}

impl RemoteDesktopApp {
    fn new() -> Self {
        let config = confy::load::<ClientConfig>("remote-desktop-client", None)
            .unwrap_or_default();
        
        Self {
            server_url_input: config.server_url.clone(),
            config,
            state: AppState::default(),
            target_id_input: String::new(),
            password_input: String::new(),
            new_password_input: String::new(),
            runtime: tokio::runtime::Runtime::new().unwrap(),
            connection: None,
            server_rx: None,
            remote_frame: None,
            frame_data: None,
            file_transfer_manager: None,
            show_settings: false,
            show_connection_details: false,
            show_file_transfer: false,
        }
    }

    fn save_config(&self) {
        let _ = confy::store("remote-desktop-client", None, &self.config);
    }

    fn connect_to_server(&mut self) {
        let server_url = self.config.server_url.clone();
        let client_id = self.config.client_id.clone();
        let password = self.config.password.clone();

        self.state.connection_state = ConnectionState::Connecting;
        self.state.error_message = None;
        self.state.reconnection_attempt = 0;

        // Convert model ReconnectionConfig to network ReconnectionConfig
        let reconnect_config = NetworkReconnectionConfig {
            max_attempts: self.config.reconnection_config.max_retries,
            initial_delay_ms: self.config.reconnection_config.base_delay_ms,
            max_delay_ms: self.config.reconnection_config.max_delay_ms,
            backoff_multiplier: 2.0,
        };

        let result = self.runtime.block_on(async {
            RelayConnection::connect_with_retry(&server_url, reconnect_config).await
        });

        match result {
            Ok((conn, rx)) => {
                let conn = Arc::new(Mutex::new(conn));

                // Start heartbeat monitor
                let conn_clone = conn.clone();
                self.runtime.spawn(async move {
                    let conn = conn_clone.lock().await;
                    conn.start_heartbeat_monitor(5000); // 5 second intervals
                });

                // Register with server
                let conn_clone = conn.clone();
                let client_id_clone = client_id.clone();
                let password_clone = password.clone();
                self.runtime.spawn(async move {
                    let conn = conn_clone.lock().await;
                    let _ = conn.send(ClientMessage::Register(client_id_clone, password_clone)).await;
                });

                self.connection = Some(conn);
                self.server_rx = Some(rx);
                self.state.connection_state = ConnectionState::Connected;
                self.state.last_connection_time = Some(chrono::Local::now().timestamp());
            }
            Err(e) => {
                self.state.error_message = Some(format!("Connection failed: {}", e));
                self.state.connection_state = ConnectionState::Disconnected;
            }
        }
    }

    fn request_connection(&mut self) {
        if let Some(ref conn) = self.connection {
            let target_id = self.target_id_input.clone();
            let password = if self.password_input.is_empty() {
                None
            } else {
                Some(self.password_input.clone())
            };
            
            let conn = conn.clone();
            self.runtime.spawn(async move {
                let conn = conn.lock().await;
                let _ = conn.send(ClientMessage::RequestConnection(target_id, password)).await;
            });
        }
    }

    fn accept_connection(&mut self, requester_id: &str) {
        if let Some(ref conn) = self.connection {
            let requester_id = requester_id.to_string();
            let conn = conn.clone();
            
            self.runtime.spawn(async move {
                let conn = conn.lock().await;
                let _ = conn.send(ClientMessage::AcceptConnection(requester_id)).await;
            });
        }
        self.state.pending_request = None;
    }

    fn reject_connection(&mut self, requester_id: &str) {
        if let Some(ref conn) = self.connection {
            let requester_id = requester_id.to_string();
            let conn = conn.clone();
            
            self.runtime.spawn(async move {
                let conn = conn.lock().await;
                let _ = conn.send(ClientMessage::RejectConnection(requester_id)).await;
            });
        }
        self.state.pending_request = None;
    }

    fn disconnect_session(&mut self) {
        if let Some(ref conn) = self.connection {
            let conn = conn.clone();
            self.runtime.spawn(async move {
                let conn = conn.lock().await;
                let _ = conn.send(ClientMessage::DisconnectSession).await;
            });
        }
        self.state.connection_state = ConnectionState::Connected;
        self.state.current_peer = None;
        self.remote_frame = None;
        self.frame_data = None;
    }

    fn reset_client_id(&mut self) {
        self.config.client_id = uuid::Uuid::new_v4().to_string();
        self.save_config();
        
        // Reconnect with new ID
        if self.connection.is_some() {
            self.connect_to_server();
        }
    }

    fn add_to_history(&mut self, client_id: &str) {
        // Check if already in history
        if !self.config.connection_history.iter().any(|e| e.client_id == client_id) {
            self.config.connection_history.push(ConnectionHistoryEntry {
                client_id: client_id.to_string(),
                last_connected: chrono::Local::now().to_string(),
                alias: None,
            });
            self.save_config();
        }
    }

    fn check_connection_health(&mut self) {
        // Only check if we have a connection
        if let Some(ref conn) = self.connection {
            let conn_clone = conn.clone();
            let state = self.state.connection_state.clone();

            // Only monitor if we're supposed to be connected
            if state == ConnectionState::Connected || state == ConnectionState::InSession {
                let is_healthy = self.runtime.block_on(async {
                    conn_clone.lock().await.is_healthy().await
                });

                // If connection is unhealthy and we're not already reconnecting, trigger reconnection
                if !is_healthy && self.state.connection_state != ConnectionState::Reconnecting {
                    log::warn!("Connection unhealthy, initiating reconnection");
                    self.state.error_message = Some("Connection lost - Network interruption detected".to_string());
                    self.attempt_reconnection();
                }
            }
        }
    }

    fn attempt_reconnection(&mut self) {
        // Don't attempt if already reconnecting or if we've exceeded max attempts
        if self.state.connection_state == ConnectionState::Reconnecting {
            return;
        }

        if self.state.reconnection_attempt >= self.config.reconnection_config.max_retries {
            self.state.error_message = Some(format!(
                "Connection failed - Unable to reconnect after {} attempts",
                self.config.reconnection_config.max_retries
            ));
            self.state.connection_state = ConnectionState::Disconnected;
            self.connection = None;
            self.server_rx = None;
            return;
        }

        // Store current session state
        let peer_before_reconnect = self.state.current_peer.clone();

        self.state.connection_state = ConnectionState::Reconnecting;
        self.state.reconnection_attempt += 1;

        // Update notification message for the attempt
        self.state.error_message = Some(format!(
            "Attempting to reconnect (attempt {}/{})",
            self.state.reconnection_attempt,
            self.config.reconnection_config.max_retries
        ));

        let server_url = self.config.server_url.clone();
        let client_id = self.config.client_id.clone();
        let password = self.config.password.clone();

        // Convert model ReconnectionConfig to network ReconnectionConfig
        let reconnect_config = NetworkReconnectionConfig {
            max_attempts: self.config.reconnection_config.max_retries - self.state.reconnection_attempt,
            initial_delay_ms: self.config.reconnection_config.base_delay_ms,
            max_delay_ms: self.config.reconnection_config.max_delay_ms,
            backoff_multiplier: 2.0,
        };

        let result = self.runtime.block_on(async {
            RelayConnection::connect_with_retry(&server_url, reconnect_config).await
        });

        match result {
            Ok((conn, rx)) => {
                let conn = Arc::new(Mutex::new(conn));

                // Start heartbeat monitor
                let conn_clone = conn.clone();
                self.runtime.spawn(async move {
                    let conn = conn_clone.lock().await;
                    conn.start_heartbeat_monitor(5000);
                });

                // Register with server
                let conn_clone = conn.clone();
                let client_id_clone = client_id.clone();
                let password_clone = password.clone();
                self.runtime.spawn(async move {
                    let conn = conn_clone.lock().await;
                    let _ = conn.send(ClientMessage::Register(client_id_clone, password_clone)).await;
                });

                self.connection = Some(conn);
                self.server_rx = Some(rx);

                // Restore connection state
                if peer_before_reconnect.is_some() {
                    self.state.connection_state = ConnectionState::InSession;
                    self.state.current_peer = peer_before_reconnect;
                } else {
                    self.state.connection_state = ConnectionState::Connected;
                }

                self.state.last_connection_time = Some(chrono::Local::now().timestamp());

                // Success notification with details
                let success_msg = if peer_before_reconnect.is_some() {
                    format!(
                        "Reconnected successfully after {} attempt(s) - Session restored",
                        self.state.reconnection_attempt
                    )
                } else {
                    format!(
                        "Reconnected successfully after {} attempt(s)",
                        self.state.reconnection_attempt
                    )
                };
                self.state.error_message = Some(success_msg);

                // Reset reconnection counter on success
                self.state.reconnection_attempt = 0;
            }
            Err(e) => {
                log::error!("Reconnection attempt {} failed: {}", self.state.reconnection_attempt, e);

                // If we've exceeded max attempts, give up
                if self.state.reconnection_attempt >= self.config.reconnection_config.max_retries {
                    self.state.error_message = Some(format!(
                        "Connection failed - Unable to reconnect after {} attempts. Reason: {}",
                        self.state.reconnection_attempt,
                        e
                    ));
                    self.state.connection_state = ConnectionState::Disconnected;
                    self.connection = None;
                    self.server_rx = None;
                } else {
                    self.state.error_message = Some(format!(
                        "Reconnection attempt {} failed ({}), retrying...",
                        self.state.reconnection_attempt,
                        e
                    ));
                }
            }
        }
    }

    fn process_server_messages(&mut self) {
        // Collect messages first to avoid borrow issues
        let messages: Vec<_> = if let Some(ref mut rx) = self.server_rx {
            let mut msgs = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                msgs.push(msg);
            }
            msgs
        } else {
            Vec::new()
        };

        // Process collected messages
        for msg in messages {
            match msg {
                ServerMessage::Registered(_) => {
                    self.state.connection_state = ConnectionState::Connected;
                }
                ServerMessage::ConnectionRequest(requester_id) => {
                    self.state.pending_request = Some(requester_id);
                }
                ServerMessage::ConnectionAccepted(peer_id) => {
                    self.state.connection_state = ConnectionState::InSession;
                    self.state.current_peer = Some(peer_id.clone());
                    self.add_to_history(&peer_id);
                }
                ServerMessage::ConnectionEstablished(peer_id) => {
                    self.state.connection_state = ConnectionState::InSession;
                    self.state.current_peer = Some(peer_id.clone());
                    self.add_to_history(&peer_id);
                }
                ServerMessage::ConnectionRejected => {
                    self.state.error_message = Some("Connection rejected".to_string());
                }
                ServerMessage::PeerDisconnected => {
                    self.state.connection_state = ConnectionState::Connected;
                    self.state.current_peer = None;
                    self.remote_frame = None;
                    self.frame_data = None;
                }
                ServerMessage::ReceiveFrame(frame) => {
                    self.frame_data = Some(frame);
                }
                ServerMessage::ReceiveInput(input) => {
                    // Process input in a separate task
                    let input = input.clone();
                    std::thread::spawn(move || {
                        let mut handler = input::InputHandler::new();
                        let _ = handler.process_input(&input);
                    });
                }
                ServerMessage::Error(err) => {
                    self.state.error_message = Some(err);
                }
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        // Only allow file drops during an active session
        if self.state.connection_state != ConnectionState::InSession {
            return;
        }

        // Get dropped files from context
        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());

        if !dropped_files.is_empty() {
            // Initialize file transfer manager if not already done
            if self.file_transfer_manager.is_none() {
                let download_dir = directories::UserDirs::new()
                    .and_then(|dirs| dirs.download_dir().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join("downloads"));

                match FileTransferManager::new(download_dir) {
                    Ok(manager) => {
                        self.file_transfer_manager = Some(manager);
                    }
                    Err(e) => {
                        self.state.error_message = Some(format!("Failed to initialize file transfer: {}", e));
                        return;
                    }
                }
            }

            // Process each dropped file
            for file in dropped_files {
                if let Some(path) = &file.path {
                    if let Some(ref mut manager) = self.file_transfer_manager {
                        // Start the file transfer
                        match manager.start_send(path.clone()) {
                            Ok(file_transfer_data) => {
                                // Send InitiateFileTransfer message to peer
                                if let Some(ref conn) = self.connection {
                                    let conn = conn.clone();
                                    let data = file_transfer_data.clone();
                                    self.runtime.spawn(async move {
                                        let conn = conn.lock().await;
                                        let _ = conn.send(ClientMessage::InitiateFileTransfer(data)).await;
                                    });
                                }

                                // Open file transfer panel to show progress
                                self.show_file_transfer = true;
                            }
                            Err(e) => {
                                let filename = path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("unknown");
                                self.state.error_message = Some(format!("Failed to queue file {}: {}", filename, e));
                            }
                        }
                    }
                }
            }
        }
    }
}

impl eframe::App for RemoteDesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process incoming messages
        self.process_server_messages();

        // Check connection health periodically
        self.check_connection_health();

        // Handle drag-and-drop file selection
        self.handle_dropped_files(ctx);

        // Request repaint for live updates
        ctx.request_repaint();

        // Top panel with app title and settings
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Remote Desktop");

                // Connection status indicator
                ui.separator();
                let (status_color, status_icon, status_text) = self.get_connection_status_indicator();
                ui.colored_label(status_color, format!("{} {}", status_icon, status_text));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙ Settings").clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    if ui.button("ℹ Details").clicked() {
                        self.show_connection_details = !self.show_connection_details;
                    }
                    if ui.button("📁 Files").clicked() {
                        self.show_file_transfer = !self.show_file_transfer;
                    }
                });
            });
        });

        // Settings window
        if self.show_settings {
            egui::Window::new("Settings")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Server URL:");
                        ui.text_edit_singleline(&mut self.server_url_input);
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("Your Password:");
                        ui.add(egui::TextEdit::singleline(&mut self.new_password_input).password(true));
                    });
                    
                    ui.horizontal(|ui| {
                        if ui.button("Save Settings").clicked() {
                            self.config.server_url = self.server_url_input.clone();
                            self.config.password = if self.new_password_input.is_empty() {
                                None
                            } else {
                                Some(self.new_password_input.clone())
                            };
                            self.save_config();
                            self.show_settings = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_settings = false;
                        }
                    });
                    
                    ui.separator();
                    
                    if ui.button("Reset Client ID").clicked() {
                        self.reset_client_id();
                    }
                    ui.label(format!("Current ID: {}", &self.config.client_id[..8]));
                });
        }

        // Connection details window
        if self.show_connection_details {
            egui::Window::new("Connection Details")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Status:");
                        let (color, icon, text) = self.get_connection_status_indicator();
                        ui.colored_label(color, format!("{} {}", icon, text));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Server:");
                        ui.monospace(&self.config.server_url);
                    });

                    if let Some(last_time) = self.state.last_connection_time {
                        let now = chrono::Local::now().timestamp();
                        let uptime_secs = now - last_time;
                        let uptime_str = if uptime_secs < 60 {
                            format!("{} seconds", uptime_secs)
                        } else if uptime_secs < 3600 {
                            format!("{} minutes", uptime_secs / 60)
                        } else {
                            format!("{} hours {} minutes", uptime_secs / 3600, (uptime_secs % 3600) / 60)
                        };

                        ui.horizontal(|ui| {
                            ui.label("Uptime:");
                            ui.label(uptime_str);
                        });
                    }

                    if let Some(ref peer) = self.state.current_peer {
                        ui.horizontal(|ui| {
                            ui.label("Connected to:");
                            ui.monospace(peer);
                        });
                    }

                    ui.separator();

                    ui.heading("Reconnection Settings");
                    ui.horizontal(|ui| {
                        ui.label("Max retries:");
                        ui.label(self.config.reconnection_config.max_retries.to_string());
                    });

                    ui.horizontal(|ui| {
                        ui.label("Base delay:");
                        ui.label(format!("{} ms", self.config.reconnection_config.base_delay_ms));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Max delay:");
                        ui.label(format!("{} ms", self.config.reconnection_config.max_delay_ms));
                    });

                    if self.state.reconnection_attempt > 0 {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Reconnection attempts:");
                            ui.label(format!("{}/{}",
                                self.state.reconnection_attempt,
                                self.config.reconnection_config.max_retries
                            ));
                        });
                    }

                    ui.add_space(10.0);

                    if ui.button("Close").clicked() {
                        self.show_connection_details = false;
                    }
                });
        }

        // File transfer window
        if self.show_file_transfer {
            egui::Window::new("File Transfers")
                .collapsible(false)
                .resizable(true)
                .default_width(400.0)
                .show(ctx, |ui| {
                    ui.heading("File Transfer Queue");
                    ui.separator();

                    // Check if we have active transfers
                    let has_transfers = false; // TODO: Check actual transfers when manager is active

                    if has_transfers {
                        // TODO: Display active transfers here
                        ui.label("Active transfers will be shown here");
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.colored_label(
                                egui::Color32::from_rgb(128, 128, 128),
                                "No active file transfers"
                            );
                            ui.add_space(20.0);
                        });
                    }

                    ui.separator();

                    if ui.button("Close").clicked() {
                        self.show_file_transfer = false;
                    }
                });
        }

        // Pending connection request dialog
        if let Some(ref requester_id) = self.state.pending_request.clone() {
            egui::Window::new("Connection Request")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Client {} wants to connect", &requester_id[..8]));
                    ui.horizontal(|ui| {
                        if ui.button("Accept").clicked() {
                            self.accept_connection(requester_id);
                        }
                        if ui.button("Reject").clicked() {
                            self.reject_connection(requester_id);
                        }
                    });
                });
        }

        // Main content
        egui::CentralPanel::default().show(ctx, |ui| {
            // Reconnection notification banner
            if self.state.connection_state == ConnectionState::Reconnecting {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(255, 200, 100))
                    .inner_margin(egui::style::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.colored_label(
                                egui::Color32::BLACK,
                                format!(
                                    "🔄 Reconnecting... (attempt {}/{})",
                                    self.state.reconnection_attempt,
                                    self.config.reconnection_config.max_retries
                                )
                            );
                        });
                    });
                ui.add_space(5.0);
            }

            // Connection restored success notification
            if let Some(ref error) = self.state.error_message {
                if error.contains("Reconnected successfully") {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(200, 255, 200))
                        .inner_margin(egui::style::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(0, 100, 0),
                                format!("✓ {}", error)
                            );
                        });
                    ui.add_space(5.0);
                }
            }

            match self.state.connection_state {
                ConnectionState::Disconnected => {
                    self.render_disconnected_view(ui);
                }
                ConnectionState::Connecting => {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                        ui.label("Connecting...");
                    });
                }
                ConnectionState::Reconnecting => {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                        ui.label(format!(
                            "Reconnecting... (attempt {}/{})",
                            self.state.reconnection_attempt,
                            self.config.reconnection_config.max_retries
                        ));
                    });
                }
                ConnectionState::Connected => {
                    self.render_connected_view(ui);
                }
                ConnectionState::InSession => {
                    self.render_session_view(ui, ctx);
                }
            }

            // Error message
            if let Some(ref error) = self.state.error_message {
                // Only show error message if it's not a success notification
                if !error.contains("Reconnected successfully") {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(255, 220, 220))
                        .inner_margin(egui::style::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.colored_label(egui::Color32::from_rgb(150, 0, 0), format!("⚠ {}", error));
                        });
                }
            }
        });
    }
}

impl RemoteDesktopApp {
    fn render_disconnected_view(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(50.0);
            
            ui.heading("Welcome to Remote Desktop");
            ui.add_space(20.0);
            
            ui.label(format!("Your ID: {}", self.config.client_id));
            ui.add_space(10.0);
            
            if ui.button("Connect to Server").clicked() {
                self.connect_to_server();
            }
        });
    }

    fn render_connected_view(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |columns| {
            // Left column - Your ID and connection
            columns[0].vertical(|ui| {
                ui.heading("Your ID");
                ui.horizontal(|ui| {
                    ui.monospace(&self.config.client_id);
                    if ui.button("📋").on_hover_text("Copy to clipboard").clicked() {
                        ui.output_mut(|o| o.copied_text = self.config.client_id.clone());
                    }
                });
                
                ui.add_space(20.0);
                
                ui.heading("Connect to Remote");
                ui.horizontal(|ui| {
                    ui.label("Target ID:");
                    ui.text_edit_singleline(&mut self.target_id_input);
                });
                ui.horizontal(|ui| {
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut self.password_input).password(true));
                });
                
                if ui.button("Connect").clicked() && !self.target_id_input.is_empty() {
                    self.request_connection();
                }
            });

            // Right column - Connection history
            columns[1].vertical(|ui| {
                ui.heading("Connection History");
                
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entry in &self.config.connection_history {
                        ui.horizontal(|ui| {
                            let short_id = &entry.client_id[..8.min(entry.client_id.len())];
                            if ui.button(format!("🔗 {}", short_id)).clicked() {
                                self.target_id_input = entry.client_id.clone();
                            }
                        });
                    }
                    
                    if self.config.connection_history.is_empty() {
                        ui.label("No previous connections");
                    }
                });
            });
        });
    }

    fn render_session_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if let Some(ref peer) = self.state.current_peer {
                ui.label(format!("Connected to: {}", &peer[..8.min(peer.len())]));
            }
            
            if ui.button("Disconnect").clicked() {
                self.disconnect_session();
            }
        });

        ui.separator();

        // Render remote frame if available
        if let Some(ref frame_data) = self.frame_data {
            if let Ok(image) = image::load_from_memory(&frame_data.image_data) {
                let rgba = image.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                
                let texture = ctx.load_texture(
                    "remote_frame",
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                
                // Calculate aspect ratio
                let available = ui.available_size();
                let aspect = frame_data.width as f32 / frame_data.height as f32;
                let size = if available.x / available.y > aspect {
                    egui::vec2(available.y * aspect, available.y)
                } else {
                    egui::vec2(available.x, available.x / aspect)
                };

                let response = ui.add(egui::Image::new(&texture).fit_to_exact_size(size).sense(egui::Sense::click_and_drag()));

                // Handle mouse input on the remote view
                if response.hovered() {
                    if let Some(pos) = response.hover_pos() {
                        let local_pos = pos - response.rect.min;
                        let scale_x = frame_data.width as f32 / size.x;
                        let scale_y = frame_data.height as f32 / size.y;
                        let remote_x = (local_pos.x * scale_x) as i32;
                        let remote_y = (local_pos.y * scale_y) as i32;

                        // Send mouse move
                        self.send_input(InputData {
                            input_type: InputType::MouseMove,
                            x: remote_x,
                            y: remote_y,
                            button: 0,
                            key_code: 0,
                            key_char: None,
                            is_key_down: false,
                        });
                    }
                }

                if response.clicked() {
                    self.send_input(InputData {
                        input_type: InputType::MouseDown,
                        x: 0,
                        y: 0,
                        button: 0,
                        key_code: 0,
                        key_char: None,
                        is_key_down: true,
                    });
                }
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("Waiting for remote screen...");
            });
        }
    }

    fn send_input(&self, input: InputData) {
        if let Some(ref conn) = self.connection {
            let conn = conn.clone();
            let input = input.clone();
            self.runtime.spawn(async move {
                let conn = conn.lock().await;
                let _ = conn.send(ClientMessage::SendInput(input)).await;
            });
        }
    }

    fn get_connection_status_indicator(&self) -> (egui::Color32, &str, &str) {
        match self.state.connection_state {
            ConnectionState::Disconnected => {
                (egui::Color32::from_rgb(180, 180, 180), "⚫", "Disconnected")
            }
            ConnectionState::Connecting => {
                (egui::Color32::from_rgb(255, 165, 0), "🟡", "Connecting")
            }
            ConnectionState::Connected => {
                (egui::Color32::from_rgb(0, 200, 0), "🟢", "Connected")
            }
            ConnectionState::Reconnecting => {
                (egui::Color32::from_rgb(255, 140, 0), "🟠", "Reconnecting")
            }
            ConnectionState::InSession => {
                (egui::Color32::from_rgb(0, 150, 255), "🔵", "In Session")
            }
        }
    }
}

