namespace RemoteDesktopServer.Models;

public class FrameData
{
    public byte[] ImageData { get; set; } = Array.Empty<byte>();
    public int Width { get; set; }
    public int Height { get; set; }
    public string Format { get; set; } = "jpeg";
    public long Timestamp { get; set; }
}
