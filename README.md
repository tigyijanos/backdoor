# Remote Desktop Solution

A TeamViewer-like remote desktop solution with a cross-platform client and server relay.

## Architecture

```
┌─────────────────┐         ┌──────────────────┐         ┌─────────────────┐
│  Client A       │◄───────►│  Relay Server    │◄───────►│  Client B       │
│  (Controller)   │   WS    │  (Docker/SignalR)│   WS    │  (Remote)       │
└─────────────────┘         └──────────────────┘         └─────────────────┘
```

## Features

### Server (ASP.NET Core SignalR)
- Real-time communication via WebSockets
- Client registration with unique IDs
- Password-protected connections
- Connection brokering between clients
- Screen frame and input relay
- Heartbeat mechanism for online status

### Client (Rust/egui)
- Cross-platform support (Windows, Linux, macOS)
- Unique machine ID (GUID) that persists across sessions
- ID reset functionality
- Password protection
- Connection history with status tracking
- Screen capture and streaming
- Remote input handling (keyboard/mouse)

## Quick Start

### Server

1. Navigate to the server directory:
   ```bash
   cd server
   ```

2. Using Docker Compose:
   ```bash
   docker-compose up -d
   ```

   Or run directly with .NET:
   ```bash
   cd RemoteDesktopServer
   dotnet run
   ```

The server will be available at `http://localhost:5000`

### Client

1. Navigate to the client directory:
   ```bash
   cd client/remote-desktop-client
   ```

2. Build the client:
   ```bash
   cargo build --release
   ```

3. Run the client:
   ```bash
   ./target/release/remote_desktop_client
   ```

## Configuration

### Client Configuration

The client stores its configuration in the system config directory:
- Linux: `~/.config/remote-desktop-client/`
- macOS: `~/Library/Application Support/remote-desktop-client/`
- Windows: `%APPDATA%\remote-desktop-client\`

Configuration includes:
- `client_id`: Unique machine identifier (auto-generated GUID)
- `password`: Optional password for incoming connections
- `server_url`: Relay server URL (default: `http://localhost:5000`)
- `connection_history`: List of previously connected clients

### Server Configuration

The server can be configured via `appsettings.json`:
- SignalR message size limits
- CORS settings
- Logging levels

## Protocol

### SignalR Hub Methods

| Method | Description |
|--------|-------------|
| `Register(clientId, password)` | Register client with the server |
| `RequestConnection(targetId, password)` | Request connection to another client |
| `AcceptConnection(requesterId)` | Accept an incoming connection request |
| `RejectConnection(requesterId)` | Reject an incoming connection request |
| `SendFrame(frameData)` | Send screen frame to connected peer |
| `SendInput(inputData)` | Send input event to connected peer |
| `GetClientStatus(clientId)` | Check if a client is online |
| `Heartbeat()` | Keep-alive signal |
| `DisconnectSession()` | End the current remote session |

### Server Events

| Event | Description |
|-------|-------------|
| `Registered` | Confirmation of successful registration |
| `ConnectionRequest` | Incoming connection request |
| `ConnectionAccepted` | Connection request was accepted |
| `ConnectionRejected` | Connection request was rejected |
| `ConnectionEstablished` | Connection is now active |
| `PeerDisconnected` | Remote peer disconnected |
| `ReceiveFrame` | Incoming screen frame |
| `ReceiveInput` | Incoming input event |

## Security Considerations

- Passwords are hashed (SHA256) before transmission
- All communication is via WebSocket (supports WSS for encryption)
- Client IDs are random GUIDs
- No data is stored on the relay server (stateless)

## Development

### Server Requirements
- .NET 8.0 SDK
- Docker (optional, for containerization)

### Client Requirements
- Rust (stable)
- System libraries:
  - Linux: `libxdo-dev`, `libxcb-*-dev`
  - macOS: Xcode command line tools
  - Windows: Visual Studio Build Tools

## License

This project is for educational purposes.
