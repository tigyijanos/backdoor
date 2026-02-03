namespace RemoteDesktopServer.Models;

public enum SessionState
{
    Active,
    Suspended
}

public class ClientInfo
{
    /// <summary>
    /// Timeout in seconds for determining if a client is online
    /// </summary>
    public const int HeartbeatTimeoutSeconds = 30;

    /// <summary>
    /// Grace period in seconds for session preservation after disconnect
    /// </summary>
    public const int SessionGracePeriodSeconds = 120;

    public string ClientId { get; set; } = string.Empty;
    public string ConnectionId { get; set; } = string.Empty;
    public string? PasswordHash { get; set; }
    public DateTime LastHeartbeat { get; set; } = DateTime.UtcNow;
    public bool IsOnline => (DateTime.UtcNow - LastHeartbeat).TotalSeconds < HeartbeatTimeoutSeconds;
    public string? ConnectedToClientId { get; set; }
    public DateTime? DisconnectedAt { get; set; }
    public SessionState SessionState { get; set; } = SessionState.Active;

    /// <summary>
    /// Determines if session can be restored after disconnection
    /// </summary>
    public bool CanRestoreSession =>
        DisconnectedAt.HasValue &&
        (DateTime.UtcNow - DisconnectedAt.Value).TotalSeconds < SessionGracePeriodSeconds &&
        SessionState == SessionState.Suspended;
}
