namespace RemoteDesktopServer.Models;

public class ClientInfo
{
    /// <summary>
    /// Timeout in seconds for determining if a client is online
    /// </summary>
    public const int HeartbeatTimeoutSeconds = 30;

    public string ClientId { get; set; } = string.Empty;
    public string ConnectionId { get; set; } = string.Empty;
    public string? PasswordHash { get; set; }
    public DateTime LastHeartbeat { get; set; } = DateTime.UtcNow;
    public bool IsOnline => (DateTime.UtcNow - LastHeartbeat).TotalSeconds < HeartbeatTimeoutSeconds;
    public string? ConnectedToClientId { get; set; }
}
