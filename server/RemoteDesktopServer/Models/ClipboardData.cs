namespace RemoteDesktopServer.Models;

public enum ClipboardType
{
    Text,
    Image
}

public class ClipboardData
{
    public ClipboardType Type { get; set; }
    public string TextData { get; set; } = string.Empty;
    public byte[] ImageData { get; set; } = Array.Empty<byte>();
    public long Timestamp { get; set; }
}
