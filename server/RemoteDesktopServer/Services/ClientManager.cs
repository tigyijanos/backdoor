using RemoteDesktopServer.Models;
using System.Collections.Concurrent;
using System.Security.Cryptography;
using System.Text;

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
            existing.ConnectionId = connectionId;
            existing.PasswordHash = passwordHash ?? existing.PasswordHash;
            existing.LastHeartbeat = DateTime.UtcNow;
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
                // Clear any active connections
                if (clientInfo.ConnectedToClientId != null)
                {
                    ClearConnection(clientId);
                }
                // Mark as offline but don't remove - they might reconnect
                clientInfo.LastHeartbeat = DateTime.MinValue;
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
        return client.PasswordHash == HashPassword(password);
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

    private static string HashPassword(string password)
    {
        using var sha256 = SHA256.Create();
        var bytes = sha256.ComputeHash(Encoding.UTF8.GetBytes(password));
        return Convert.ToBase64String(bytes);
    }
}
