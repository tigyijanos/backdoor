namespace RemoteDesktopServer.Models;

public class ClientInfo
{
    public string ClientId { get; set; } = string.Empty;
    public string ConnectionId { get; set; } = string.Empty;
    public string? PasswordHash { get; set; }
    public DateTime LastHeartbeat { get; set; } = DateTime.UtcNow;
    public bool IsOnline => (DateTime.UtcNow - LastHeartbeat).TotalSeconds < 30;
    public string? ConnectedToClientId { get; set; }
}
