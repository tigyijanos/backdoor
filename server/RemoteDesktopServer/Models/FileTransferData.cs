namespace RemoteDesktopServer.Models;

public class FileTransferData
{
    public string TransferId { get; set; } = string.Empty;
    public string Filename { get; set; } = string.Empty;
    public long FileSize { get; set; }
    public int TotalChunks { get; set; }
}
