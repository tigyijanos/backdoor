namespace RemoteDesktopServer.Models;

public class InputData
{
    public InputType Type { get; set; }
    public int X { get; set; }
    public int Y { get; set; }
    public int Button { get; set; }
    public int KeyCode { get; set; }
    public string? KeyChar { get; set; }
    public bool IsKeyDown { get; set; }
}

public enum InputType
{
    MouseMove,
    MouseDown,
    MouseUp,
    MouseScroll,
    KeyDown,
    KeyUp
}
