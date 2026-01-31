mod capture;
mod input;
mod models;
mod network;

use anyhow::Result;
use eframe::egui;
use models::{AppState, ClientConfig, ConnectionHistoryEntry, ConnectionState, InputData, InputType};
use network::{ClientMessage, RelayConnection, ServerMessage};
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
    
    // Settings panel
    show_settings: bool,
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
            show_settings: false,
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

        let result = self.runtime.block_on(async {
            RelayConnection::connect(&server_url).await
        });

        match result {
            Ok((conn, rx)) => {
                let conn = Arc::new(Mutex::new(conn));
                
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
}

impl eframe::App for RemoteDesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process incoming messages
        self.process_server_messages();

        // Request repaint for live updates
        ctx.request_repaint();

        // Top panel with app title and settings
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Remote Desktop");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙ Settings").clicked() {
                        self.show_settings = !self.show_settings;
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
                ConnectionState::Connected => {
                    self.render_connected_view(ui);
                }
                ConnectionState::InSession => {
                    self.render_session_view(ui, ctx);
                }
            }

            // Error message
            if let Some(ref error) = self.state.error_message {
                ui.colored_label(egui::Color32::RED, error);
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
}

