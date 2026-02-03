using RemoteDesktopServer.Models;
using System.Collections.Concurrent;

namespace RemoteDesktopServer.Services;

public interface IClientManager
{
    void RegisterClient(string clientId, string connectionId, string? password);
    void UnregisterClient(string connectionId);
    ClientInfo? GetClientByClientId(string clientId);
    ClientInfo? GetClientByConnectionId(string connectionId);
    bool ValidatePassword(string clientId, string password);
    void UpdateHeartbeat(string connectionId);
    void SetConnection(string clientId1, string clientId2);
    void ClearConnection(string clientId);
    IEnumerable<ClientInfo> GetAllClients();
    void CleanupExpiredSessions();
}

public class ClientManager : IClientManager
{
    private readonly ConcurrentDictionary<string, ClientInfo> _clientsByClientId = new();
    private readonly ConcurrentDictionary<string, string> _connectionToClient = new();

    public void RegisterClient(string clientId, string connectionId, string? password)
    {
        var passwordHash = password != null ? HashPassword(password) : null;

        var clientInfo = new ClientInfo
        {
            ClientId = clientId,
            ConnectionId = connectionId,
            PasswordHash = passwordHash,
            LastHeartbeat = DateTime.UtcNow
        };

        _clientsByClientId.AddOrUpdate(clientId, clientInfo, (_, existing) =>
        {
            // Check if we can restore a suspended session
            if (existing.CanRestoreSession)
            {
                // Restore session: keep ConnectedToClientId and restore active state
                existing.ConnectionId = connectionId;
                existing.PasswordHash = passwordHash ?? existing.PasswordHash;
                existing.LastHeartbeat = DateTime.UtcNow;
                existing.SessionState = SessionState.Active;
                existing.DisconnectedAt = null;
                // Keep existing.ConnectedToClientId - preserved during reconnection
            }
            else
            {
                // Session expired or new connection: reset connection state
                existing.ConnectionId = connectionId;
                existing.PasswordHash = passwordHash ?? existing.PasswordHash;
                existing.LastHeartbeat = DateTime.UtcNow;
                existing.SessionState = SessionState.Active;
                existing.DisconnectedAt = null;
                existing.ConnectedToClientId = null; // Clear expired session connections
            }
            return existing;
        });

        _connectionToClient[connectionId] = clientId;
    }

    public void UnregisterClient(string connectionId)
    {
        if (_connectionToClient.TryRemove(connectionId, out var clientId))
        {
            if (_clientsByClientId.TryGetValue(clientId, out var clientInfo))
            {
                // Suspend session instead of terminating - preserve for grace period
                clientInfo.SessionState = SessionState.Suspended;
                clientInfo.DisconnectedAt = DateTime.UtcNow;
                clientInfo.LastHeartbeat = DateTime.MinValue;
                // Don't clear ConnectedToClientId - preserve it for potential reconnection
                // The session will be restored if client reconnects within grace period
            }
        }
    }

    public ClientInfo? GetClientByClientId(string clientId)
    {
        _clientsByClientId.TryGetValue(clientId, out var client);
        return client;
    }

    public ClientInfo? GetClientByConnectionId(string connectionId)
    {
        if (_connectionToClient.TryGetValue(connectionId, out var clientId))
        {
            return GetClientByClientId(clientId);
        }
        return null;
    }

    public bool ValidatePassword(string clientId, string password)
    {
        var client = GetClientByClientId(clientId);
        if (client == null) return false;
        if (client.PasswordHash == null) return true; // No password set
        return VerifyPassword(password, client.PasswordHash);
    }

    public void UpdateHeartbeat(string connectionId)
    {
        var client = GetClientByConnectionId(connectionId);
        if (client != null)
        {
            client.LastHeartbeat = DateTime.UtcNow;
        }
    }

    public void SetConnection(string clientId1, string clientId2)
    {
        var client1 = GetClientByClientId(clientId1);
        var client2 = GetClientByClientId(clientId2);
        
        if (client1 != null) client1.ConnectedToClientId = clientId2;
        if (client2 != null) client2.ConnectedToClientId = clientId1;
    }

    public void ClearConnection(string clientId)
    {
        var client = GetClientByClientId(clientId);
        if (client?.ConnectedToClientId != null)
        {
            var otherClient = GetClientByClientId(client.ConnectedToClientId);
            if (otherClient != null)
            {
                otherClient.ConnectedToClientId = null;
            }
            client.ConnectedToClientId = null;
        }
    }

    public IEnumerable<ClientInfo> GetAllClients()
    {
        return _clientsByClientId.Values;
    }

    public void CleanupExpiredSessions()
    {
        var now = DateTime.UtcNow;
        var expiredClients = _clientsByClientId.Values
            .Where(c => c.SessionState == SessionState.Suspended &&
                       c.DisconnectedAt.HasValue &&
                       (now - c.DisconnectedAt.Value).TotalSeconds >= ClientInfo.SessionGracePeriodSeconds)
            .ToList();

        foreach (var client in expiredClients)
        {
            _clientsByClientId.TryRemove(client.ClientId, out _);
        }
    }

    private static string HashPassword(string password)
    {
        return BCrypt.Net.BCrypt.HashPassword(password);
    }

    private static bool VerifyPassword(string password, string hash)
    {
        return BCrypt.Net.BCrypt.Verify(password, hash);
    }
}
