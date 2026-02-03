using RemoteDesktopServer.Hubs;
using RemoteDesktopServer.Services;

var builder = WebApplication.CreateBuilder(args);

// Add services
builder.Services.AddSingleton<IClientManager, ClientManager>();

// Add SignalR with MessagePack for better performance
builder.Services.AddSignalR(options =>
{
    options.MaximumReceiveMessageSize = 10 * 1024 * 1024; // 10MB for screen frames
    options.EnableDetailedErrors = builder.Environment.IsDevelopment();
});

// Add CORS for cross-platform clients
builder.Services.AddCors(options =>
{
    options.AddDefaultPolicy(policy =>
    {
        if (builder.Environment.IsDevelopment())
        {
            // Allow any origin in development
            policy.AllowAnyOrigin()
                  .AllowAnyMethod()
                  .AllowAnyHeader();
        }
        else
        {
            // In production, configure allowed origins via configuration
            var allowedOrigins = builder.Configuration.GetSection("AllowedOrigins").Get<string[]>() 
                                 ?? Array.Empty<string>();
            if (allowedOrigins.Length > 0)
            {
                policy.WithOrigins(allowedOrigins)
                      .AllowAnyMethod()
                      .AllowAnyHeader()
                      .AllowCredentials();
            }
            else
            {
                // Fallback: allow any origin but log a warning
                policy.AllowAnyOrigin()
                      .AllowAnyMethod()
                      .AllowAnyHeader();
            }
        }
    });
});

// Health checks
builder.Services.AddHealthChecks();

var app = builder.Build();

app.UseCors();

// Health check endpoint
app.MapHealthChecks("/health");

// SignalR hub endpoint
app.MapHub<RemoteDesktopHub>("/hub");

// Server info endpoint
app.MapGet("/", () => new 
{
    Name = "Remote Desktop Relay Server",
    Version = "1.0.0",
    Status = "Running",
    HubEndpoint = "/hub"
});

app.Run();
