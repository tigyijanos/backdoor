using Microsoft.AspNetCore.SignalR;
using RemoteDesktopServer.Models;
using RemoteDesktopServer.Services;

namespace RemoteDesktopServer.Hubs;

public class RemoteDesktopHub : Hub
{
    private readonly IClientManager _clientManager;
    private readonly ILogger<RemoteDesktopHub> _logger;

    public RemoteDesktopHub(IClientManager clientManager, ILogger<RemoteDesktopHub> logger)
    {
        _clientManager = clientManager;
        _logger = logger;
    }

    public override async Task OnDisconnectedAsync(Exception? exception)
    {
        var client = _clientManager.GetClientByConnectionId(Context.ConnectionId);
        if (client != null)
        {
            _logger.LogInformation("Client {ClientId} disconnected", client.ClientId);
            
            // Notify connected peer if any
            if (client.ConnectedToClientId != null)
            {
                var peer = _clientManager.GetClientByClientId(client.ConnectedToClientId);
                if (peer != null)
                {
                    await Clients.Client(peer.ConnectionId).SendAsync("PeerDisconnected");
                }
            }
            
            _clientManager.UnregisterClient(Context.ConnectionId);
        }
        await base.OnDisconnectedAsync(exception);
    }

    /// <summary>
    /// Register client with its unique ID and optional password
    /// </summary>
    public async Task<bool> Register(string clientId, string? password)
    {
        _logger.LogInformation("Client registering: {ClientId}", clientId);

        // Register client - ClientManager will restore session if within grace period
        _clientManager.RegisterClient(clientId, Context.ConnectionId, password);

        // Check if this was a reconnection with session restoration
        var client = _clientManager.GetClientByClientId(clientId);
        if (client?.ConnectedToClientId != null)
        {
            // Session was restored - notify both parties
            _logger.LogInformation("Session restored for {ClientId}, connected to {PeerId}",
                clientId, client.ConnectedToClientId);

            // Notify the reconnecting client
            await Clients.Caller.SendAsync("ReconnectionSuccessful", client.ConnectedToClientId);

            // Notify the peer that the session resumed
            var peer = _clientManager.GetClientByClientId(client.ConnectedToClientId);
            if (peer != null && peer.IsOnline)
            {
                await Clients.Client(peer.ConnectionId).SendAsync("SessionRestored", clientId);
            }
        }
        else
        {
            // Normal registration without session restoration
            await Clients.Caller.SendAsync("Registered", clientId);
        }

        return true;
    }

    /// <summary>
    /// Request connection to another client
    /// </summary>
    public async Task<string> RequestConnection(string targetClientId, string? password)
    {
        var requester = _clientManager.GetClientByConnectionId(Context.ConnectionId);
        if (requester == null)
        {
            return "NOT_REGISTERED";
        }

        var target = _clientManager.GetClientByClientId(targetClientId);
        if (target == null || !target.IsOnline)
        {
            return "TARGET_OFFLINE";
        }

        if (!_clientManager.ValidatePassword(targetClientId, password ?? ""))
        {
            return "INVALID_PASSWORD";
        }

        // Notify target about connection request
        await Clients.Client(target.ConnectionId).SendAsync("ConnectionRequest", requester.ClientId);
        return "REQUEST_SENT";
    }

    /// <summary>
    /// Accept incoming connection request
    /// </summary>
    public async Task AcceptConnection(string requesterId)
    {
        var target = _clientManager.GetClientByConnectionId(Context.ConnectionId);
        var requester = _clientManager.GetClientByClientId(requesterId);

        if (target == null || requester == null)
        {
            _logger.LogWarning("AcceptConnection failed: client not found");
            return;
        }

        _clientManager.SetConnection(target.ClientId, requester.ClientId);
        
        await Clients.Client(requester.ConnectionId).SendAsync("ConnectionAccepted", target.ClientId);
        await Clients.Caller.SendAsync("ConnectionEstablished", requester.ClientId);
        
        _logger.LogInformation("Connection established between {Client1} and {Client2}", 
            target.ClientId, requester.ClientId);
    }

    /// <summary>
    /// Reject incoming connection request
    /// </summary>
    public async Task RejectConnection(string requesterId)
    {
        var requester = _clientManager.GetClientByClientId(requesterId);
        if (requester != null)
        {
            await Clients.Client(requester.ConnectionId).SendAsync("ConnectionRejected");
        }
    }

    /// <summary>
    /// Send screen frame to connected peer
    /// </summary>
    public async Task SendFrame(FrameData frameData)
    {
        var sender = _clientManager.GetClientByConnectionId(Context.ConnectionId);
        if (sender?.ConnectedToClientId == null) return;

        var peer = _clientManager.GetClientByClientId(sender.ConnectedToClientId);
        if (peer != null)
        {
            await Clients.Client(peer.ConnectionId).SendAsync("ReceiveFrame", frameData);
        }
    }

    /// <summary>
    /// Send input event to connected peer
    /// </summary>
    public async Task SendInput(InputData inputData)
    {
        var sender = _clientManager.GetClientByConnectionId(Context.ConnectionId);
        if (sender?.ConnectedToClientId == null) return;

        var peer = _clientManager.GetClientByClientId(sender.ConnectedToClientId);
        if (peer != null)
        {
            await Clients.Client(peer.ConnectionId).SendAsync("ReceiveInput", inputData);
        }
    }

    /// <summary>
    /// Check if a client is online
    /// </summary>
    public Task<bool> GetClientStatus(string clientId)
    {
        var client = _clientManager.GetClientByClientId(clientId);
        return Task.FromResult(client?.IsOnline ?? false);
    }

    /// <summary>
    /// Heartbeat to keep connection alive
    /// </summary>
    public Task Heartbeat()
    {
        _clientManager.UpdateHeartbeat(Context.ConnectionId);
        return Task.CompletedTask;
    }

    /// <summary>
    /// Disconnect from current remote session
    /// </summary>
    public async Task DisconnectSession()
    {
        var client = _clientManager.GetClientByConnectionId(Context.ConnectionId);
        if (client?.ConnectedToClientId != null)
        {
            var peer = _clientManager.GetClientByClientId(client.ConnectedToClientId);
            if (peer != null)
            {
                await Clients.Client(peer.ConnectionId).SendAsync("PeerDisconnected");
            }
            _clientManager.ClearConnection(client.ClientId);
        }
    }
}
