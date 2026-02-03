namespace RemoteDesktopServer.Models;

public class FileChunk
{
    public string TransferId { get; set; } = string.Empty;
    public int ChunkIndex { get; set; }
    public byte[] Data { get; set; } = Array.Empty<byte>();
    public string Checksum { get; set; } = string.Empty;
}
