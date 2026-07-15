#nullable enable
using Godot;
using Google.Protobuf;
using System;
using System.Text.Json;
using WeaponsMastersClient.Autoload;
using WeaponsMastersClient.Game;
using WeaponsMastersClient.Prediction;
using Wm;

namespace WeaponsMastersClient.Network;

/// <summary>
/// Gerencia a conexão de rede (WebTransport ou WebSocket) e o ciclo de input/snapshot.
/// O entity_id é atribuído pelo servidor — este nó não gera nem envia IDs locais.
/// </summary>
public partial class NetworkManager : Node
{
    [Export] public string ServerUrl { get; set; } = "https://localhost:4433";
    [Export] public string WebSocketFallbackUrl { get; set; } = "ws://127.0.0.1:8080";
    [Export(PropertyHint.MultilineText)] public string ServerCertificateHashBytes { get; set; } = "";
    [Export] public NodePath InputSenderPath { get; set; } = new("");
    [Export] public NodePath PacketHandlerPath { get; set; } = new("");
    [Export] public NodePath LocalPlayerPath { get; set; } = new("");
    [Export] public NodePath LocalCameraPath { get; set; } = new("");

    private InputSender? _inputSender;
    private PacketHandler? _packetHandler;
    private ClientPrediction? _localPlayer;
    private Camera3D? _localCamera;
    private WebSocketPeer? _webSocketPeer;
    private readonly WorldEntryService _worldEntry = new();
    private bool _authSent;
    private double _lastReAuthAttemptMs;

    public GameConnectionState ConnectionState { get; private set; } =
        GameConnectionState.Disconnected;

    public override void _Ready()
    {
        _inputSender  = GetNodeOrNull<InputSender>(InputSenderPath);
        _packetHandler = GetNodeOrNull<PacketHandler>(PacketHandlerPath);
        _localPlayer  = GetNodeOrNull<ClientPrediction>(LocalPlayerPath);
        _localCamera  = GetNodeOrNull<Camera3D>(LocalCameraPath);

        _packetHandler?.ConfigureWorldEntryService(_worldEntry);
        _worldEntry.WorldEntered += OnWorldEntered;
        LockLocalPlayerUntilWorldEntry();
        ConnectionState = GameConnectionState.Connecting;

        StartWebSocketFallback();
        StartWebTransport();
    }

    public override void _Process(double delta)
    {
        PollWebSocketFallback();
        EnsureWebTransportAuthSent();
        DrainWebTransportSnapshots();
    }

    public override void _PhysicsProcess(double delta)
    {
        if (ConnectionState != GameConnectionState.InWorld
            || !_worldEntry.IsReady
            || _inputSender is null
            || _localPlayer is null)
        {
            return;
        }

        HandleDodgeInput();
        HandleSkillInputs();
        HandleMovementInput();
    }

    private void HandleDodgeInput()
    {
        if (!Input.IsActionJustPressed("dodge") || _inputSender is null || _localPlayer is null)
        {
            return;
        }

        var dodgeInput = _inputSender.CaptureDodgeInput();
        _localPlayer.ApplyLocalDodge(_inputSender.CurrentDirection);
        SendInput(dodgeInput.ToByteArray());
    }

    private void HandleSkillInputs()
    {
        if (Input.IsActionJustPressed("skill_golpe"))
        {
            TrySendSkillInput(skillId: 1);
        }
        if (Input.IsActionJustPressed("skill_disparo"))
        {
            TrySendSkillInput(skillId: 2);
        }
    }

    private void HandleMovementInput()
    {
        if (_inputSender is null || _localPlayer is null)
        {
            return;
        }

        if (Input.IsActionJustPressed("target_next"))
        {
            _packetHandler?.SelectNextTarget();
        }

        var movementInput = _inputSender.CaptureMovementInput();
        _localPlayer.ApplyLocalInput(movementInput);
        SendInput(movementInput.ToByteArray());
    }

    public void ReceiveSnapshot(byte[] payload)
    {
        ReceiveServerPacket(payload);
    }

    private void ReceiveServerPacket(byte[] payload)
    {
        WorldSnapshot snapshot;
        try
        {
            snapshot = WorldSnapshot.Parser.ParseFrom(payload);
        }
        catch (InvalidProtocolBufferException ex)
        {
            GD.PushWarning($"[NetworkManager] Invalid server packet: {ex.Message}");
            return;
        }

        if (snapshot.SessionReauthChallenge != null)
        {
            HandleSessionReAuthChallenge(snapshot.SessionReauthChallenge.DeadlineSecs);
        }

        if (snapshot.SessionReauthResult != null)
        {
            HandleSessionReAuthResult(snapshot.SessionReauthResult);
        }
        else if (_lastReAuthAttemptMs > 0 && HasGameplayState(snapshot))
        {
            // ReAuth concluído — servidor parou de anexar o challenge.
            _lastReAuthAttemptMs = 0;
            GD.Print("[NetworkManager] Session revalidated — gameplay resumed");
        }

        if (_packetHandler is null)
        {
            return;
        }

        if (HasGameplayState(snapshot))
        {
            _packetHandler.ApplySnapshot(snapshot);
        }
        else if (snapshot.LocalEntityId != 0)
        {
            _packetHandler.SetLocalEntityId(snapshot.LocalEntityId);
        }
    }

    private static bool HasGameplayState(WorldSnapshot snapshot)
    {
        return snapshot.Tick > 0
            || snapshot.Entities.Count > 0
            || snapshot.MobEntities.Count > 0
            || snapshot.CombatEvents.Count > 0
            || snapshot.LevelUpEvents.Count > 0
            || snapshot.LootDrops.Count > 0;
    }

    private void HandleSessionReAuthResult(SessionReAuthResult result)
    {
        if (result.Success)
        {
            Session.Instance?.UpdateTokens(result.AccessToken, result.RefreshToken);
            _lastReAuthAttemptMs = 0;
            GD.Print("[NetworkManager] Session revalidated — tokens rotated");
            return;
        }

        GD.PushWarning($"[NetworkManager] Session reauth failed: {result.ErrorMessage}");
    }

    private void HandleSessionReAuthChallenge(uint deadlineSecs)
    {
        var now = Time.GetTicksMsec();
        // Debounce: snapshots periódicos repetem o challenge enquanto ReAuth está ativo.
        if (now - _lastReAuthAttemptMs < 2000)
        {
            return;
        }

        _lastReAuthAttemptMs = now;
        GD.Print($"[NetworkManager] ReAuth challenge — {deadlineSecs}s to revalidate session");
        SendSessionReAuth();
    }

    private void TrySendSkillInput(uint skillId)
    {
        if (_inputSender is null || _packetHandler is null)
        {
            return;
        }
        if (_packetHandler.SelectedTargetEntityId == 0)
        {
            return;
        }

        var skillInput = _inputSender.CaptureSkillInput(skillId, _packetHandler.SelectedTargetEntityId);
        SendInput(skillInput.ToByteArray());
    }

    private void SendInput(byte[] payload)
    {
        if (OS.HasFeature("web"))
        {
            SendInputViaWebTransport(payload);
        }
        else
        {
            SendInputViaWebSocket(payload);
        }
    }

    private void SendInputViaWebSocket(byte[] payload)
    {
        if (_webSocketPeer?.GetReadyState() == WebSocketPeer.State.Open)
        {
            _webSocketPeer.PutPacket(payload);
        }
    }

    private static ClientPlatform DetectClientPlatform()
    {
        if (OS.HasFeature("web"))
        {
            return ClientPlatform.Web;
        }
        if (OS.HasFeature("mobile") || OS.HasFeature("android") || OS.HasFeature("ios"))
        {
            return ClientPlatform.Mobile;
        }
        return ClientPlatform.Pc;
    }

    private void SendGameAuthPacket()
    {
        var token = Session.Instance?.Token ?? "";
        var authPacket = new GameAuthPacket
        {
            Token = token,
            ClientPlatform = DetectClientPlatform(),
        };
        ConnectionState = GameConnectionState.Authenticating;
        SendInput(authPacket.ToByteArray());
        _worldEntry.AuthenticationSent();
        ConnectionState = GameConnectionState.WaitingForWorld;
        GD.Print($"[NetworkManager] GameAuthPacket sent (authenticated: {token.Length > 0}, platform: {authPacket.ClientPlatform})");
    }

    private void LockLocalPlayerUntilWorldEntry()
    {
        SetPhysicsProcess(false);

        if (_localPlayer is not null)
        {
            _localPlayer.Visible = false;
            _localPlayer.SetProcess(false);
            _localPlayer.SetPhysicsProcess(false);
        }

        if (_inputSender is not null)
        {
            _inputSender.SetProcess(false);
            _inputSender.SetPhysicsProcess(false);
        }

        if (_localCamera is not null)
        {
            _localCamera.Current = false;
        }
    }

    private void OnWorldEntered()
    {
        // PacketHandler applies authoritative transform/health before raising
        // this event. Only then may the player become visible and interactive.
        if (_localPlayer is not null)
        {
            _localPlayer.Visible = true;
            _localPlayer.SetProcess(true);
            _localPlayer.SetPhysicsProcess(true);
        }

        if (_inputSender is not null)
        {
            _inputSender.SetProcess(true);
            _inputSender.SetPhysicsProcess(true);
        }

        if (_localCamera is not null)
        {
            _localCamera.Current = true;
        }

        ConnectionState = GameConnectionState.InWorld;
        SetPhysicsProcess(true);
        GD.Print("[NetworkManager] Authoritative world entry confirmed; input enabled");
    }

    public override void _ExitTree()
    {
        _worldEntry.WorldEntered -= OnWorldEntered;
        ConnectionState = GameConnectionState.Disconnected;
    }

    /// <summary>
    /// Revalidates session after network handoff (Mobile/Web) using the rotating refresh token.
    /// </summary>
    public void SendSessionReAuth()
    {
        var refreshToken = Session.Instance?.RefreshToken ?? "";
        if (refreshToken.Length == 0)
        {
            GD.PushWarning("[NetworkManager] SendSessionReAuth: no refresh token available");
            return;
        }
        var reauthPacket = new SessionReAuthPacket { RefreshToken = refreshToken };
        SendInput(reauthPacket.ToByteArray());
        GD.Print("[NetworkManager] SessionReAuthPacket sent");
    }

    private void SendInputViaWebTransport(byte[] payload)
    {
        var encodedPayload = JsonSerializer.Serialize(Convert.ToBase64String(payload));
        JavaScriptBridge.Eval($"window.__wmSendInput && window.__wmSendInput({encodedPayload});", true);
    }

    private void StartWebSocketFallback()
    {
        if (OS.HasFeature("web"))
        {
            return;
        }

        _webSocketPeer = new WebSocketPeer();
        var error = _webSocketPeer.ConnectToUrl(WebSocketFallbackUrl);
        if (error != Error.Ok)
        {
            GD.PushError($"Failed to connect WebSocket fallback: {error}");
        }
    }

    private void PollWebSocketFallback()
    {
        if (OS.HasFeature("web") || _webSocketPeer is null)
        {
            return;
        }

        _webSocketPeer.Poll();

        // Send GameAuthPacket as the very first binary packet once the connection is open.
        if (!_authSent && _webSocketPeer.GetReadyState() == WebSocketPeer.State.Open)
        {
            _authSent = true;
            SendGameAuthPacket();
        }

        while (_webSocketPeer.GetAvailablePacketCount() > 0)
        {
            ReceiveSnapshot(_webSocketPeer.GetPacket());
        }
    }

    private void StartWebTransport()
    {
        if (!OS.HasFeature("web"))
        {
            return;
        }

        var encodedUrl       = JsonSerializer.Serialize(ServerUrl);
        var encodedHashBytes = JsonSerializer.Serialize(ServerCertificateHashBytes.Trim());

        JavaScriptBridge.Eval($$"""
            (() => {
                if (window.__wmTransportStarted) return;
                window.__wmTransportStarted = true;
                window.__wmSnapshots = [];

                const decodeBase64 = (b64) => {
                    const bin = atob(b64);
                    const bytes = new Uint8Array(bin.length);
                    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
                    return bytes;
                };
                const encodeBase64 = (bytes) => {
                    let bin = "";
                    for (const b of bytes) bin += String.fromCharCode(b);
                    return btoa(bin);
                };

                window.__wmSendInput = (b64Payload) => {
                    if (!window.__wmDatagramWriter) return false;
                    window.__wmDatagramWriter.write(decodeBase64(b64Payload));
                    return true;
                };
                window.__wmDrainSnapshots = () => {
                    const snaps = window.__wmSnapshots.join("\n");
                    window.__wmSnapshots.length = 0;
                    return snaps;
                };

                const parseHashBytes = (raw) => {
                    if (!raw) return null;
                    const bytes = raw.replace(/[\[\]\s]/g, "").split(",")
                        .filter(Boolean).map(Number);
                    return bytes.length === 32 ? new Uint8Array(bytes) : null;
                };

                (async () => {
                    const hashBytes = parseHashBytes({{encodedHashBytes}});
                    const opts = hashBytes
                        ? { serverCertificateHashes: [{ algorithm: "sha-256", value: hashBytes.buffer }] }
                        : undefined;
                    const transport = new WebTransport({{encodedUrl}}, opts);
                    await transport.ready;
                    window.__wmTransport = transport;
                    window.__wmDatagramWriter = transport.datagrams.writable.getWriter();
                    const reader = transport.datagrams.readable.getReader();
                    while (true) {
                        const { done, value } = await reader.read();
                        if (done) break;
                        window.__wmSnapshots.push(encodeBase64(value));
                    }
                })().catch((err) => {
                    window.__wmTransportError = String(err);
                    console.error("WebTransport error:", err);
                });
            })();
            """, true);
    }

    private void EnsureWebTransportAuthSent()
    {
        if (_authSent || !OS.HasFeature("web"))
        {
            return;
        }

        var ready = JavaScriptBridge.Eval(
            "window.__wmDatagramWriter ? '1' : '0';",
            true
        ).AsString();

        if (ready != "1")
        {
            return;
        }

        _authSent = true;
        SendGameAuthPacket();
    }

    private void DrainWebTransportSnapshots()
    {
        if (!OS.HasFeature("web"))
        {
            return;
        }

        var rawSnapshots = JavaScriptBridge.Eval(
            "window.__wmDrainSnapshots ? window.__wmDrainSnapshots() : '';",
            true
        ).AsString();

        if (string.IsNullOrWhiteSpace(rawSnapshots))
        {
            return;
        }

        foreach (var rawSnapshot in rawSnapshots.Split('\n', StringSplitOptions.RemoveEmptyEntries))
        {
            ReceiveSnapshot(Convert.FromBase64String(rawSnapshot));
        }
    }
}
