using Godot;
using Google.Protobuf;
using System;
using System.Text.Json;
using WeaponsMastersClient.Prediction;
using Wm;

namespace WeaponsMastersClient.Network;

public partial class NetworkManager : Node
{
    [Export] public string ServerUrl { get; set; } = "https://localhost:4433";
    [Export] public string WebSocketFallbackUrl { get; set; } = "ws://127.0.0.1:8080";
    [Export(PropertyHint.MultilineText)] public string ServerCertificateHashBytes { get; set; } = "";
    [Export] public NodePath InputSenderPath { get; set; } = new("");
    [Export] public NodePath PacketHandlerPath { get; set; } = new("");
    [Export] public NodePath LocalPlayerPath { get; set; } = new("");

    private InputSender? _inputSender;
    private PacketHandler? _packetHandler;
    private ClientPrediction? _localPlayer;
    private WebSocketPeer? _webSocketPeer;
    private uint _localEntityId;
    private uint _clientTick;
    private bool _transportWarningShown;

    public override void _Ready()
    {
        _inputSender = GetNodeOrNull<InputSender>(InputSenderPath);
        _packetHandler = GetNodeOrNull<PacketHandler>(PacketHandlerPath);
        _localPlayer = GetNodeOrNull<ClientPrediction>(LocalPlayerPath);
        _localEntityId = GenerateEntityId();

        _inputSender?.SetEntityId(_localEntityId);
        _packetHandler?.SetLocalEntityId(_localEntityId);

        StartWebTransport();
        StartWebSocketFallback();
    }

    public override void _Process(double delta)
    {
        PollWebSocketFallback();
        DrainSnapshots();
    }

    public override void _PhysicsProcess(double delta)
    {
        if (_inputSender is null || _localPlayer is null)
        {
            return;
        }

        _clientTick++;

        if (Input.IsActionJustPressed("target_next"))
        {
            _packetHandler?.SelectNextTarget();
        }

        if (Input.IsActionJustPressed("dodge"))
        {
            var dodgeInput = _inputSender.CaptureDodgeInput(_clientTick);
            _localPlayer.ApplyLocalDodge(_inputSender.CurrentDirection);
            SendInput(dodgeInput.ToByteArray());
        }

        if (Input.IsActionJustPressed("skill_golpe"))
        {
            SendSkillInput(1);
        }

        if (Input.IsActionJustPressed("skill_disparo"))
        {
            SendSkillInput(2);
        }

        var input = _inputSender.CaptureMovementInput(_clientTick);
        _localPlayer.ApplyLocalInput(input);
        SendInput(input.ToByteArray());
    }

    public void ReceiveSnapshot(byte[] payload)
    {
        if (_packetHandler is null)
        {
            return;
        }

        var snapshot = WorldSnapshot.Parser.ParseFrom(payload);
        _packetHandler.ApplySnapshot(snapshot);
    }

    private void SendInput(byte[] payload)
    {
        if (!OS.HasFeature("web"))
        {
            if (_webSocketPeer?.GetReadyState() == WebSocketPeer.State.Open)
            {
                _webSocketPeer.PutPacket(payload);
            }
            else if (!_transportWarningShown)
            {
                _transportWarningShown = true;
                GD.PushWarning("WebSocket fallback is not connected yet.");
            }

            return;
        }

        var encodedPayload = JsonSerializer.Serialize(Convert.ToBase64String(payload));
        JavaScriptBridge.Eval($"window.__wmSendInput && window.__wmSendInput({encodedPayload});", true);
    }

    private void SendSkillInput(uint skillId)
    {
        if (_inputSender is null || _packetHandler is null || _packetHandler.SelectedTargetEntityId == 0)
        {
            return;
        }

        var skillInput = _inputSender.CaptureSkillInput(
            skillId,
            _packetHandler.SelectedTargetEntityId,
            _clientTick
        );
        SendInput(skillInput.ToByteArray());
    }

    private void StartWebTransport()
    {
        if (!OS.HasFeature("web"))
        {
            return;
        }

        var encodedUrl = JsonSerializer.Serialize(ServerUrl);
        var encodedHashBytes = JsonSerializer.Serialize(ServerCertificateHashBytes.Trim());
        JavaScriptBridge.Eval($$"""
            (() => {
                if (window.__wmTransportStarted) return;

                window.__wmTransportStarted = true;
                window.__wmSnapshots = [];

                const decodeBase64 = (base64) => {
                    const binary = atob(base64);
                    const bytes = new Uint8Array(binary.length);
                    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
                    return bytes;
                };

                const encodeBase64 = (bytes) => {
                    let binary = "";
                    for (const byte of bytes) binary += String.fromCharCode(byte);
                    return btoa(binary);
                };

                window.__wmSendInput = (base64Payload) => {
                    if (!window.__wmDatagramWriter) return false;
                    window.__wmDatagramWriter.write(decodeBase64(base64Payload));
                    return true;
                };

                window.__wmDrainSnapshots = () => {
                    const snapshots = window.__wmSnapshots.join("\n");
                    window.__wmSnapshots.length = 0;
                    return snapshots;
                };

                const parseHashBytes = (raw) => {
                    if (!raw) return null;
                    const bytes = raw
                        .replace(/[\[\]\s]/g, "")
                        .split(",")
                        .filter(Boolean)
                        .map((value) => Number(value));
                    return bytes.length === 32 ? new Uint8Array(bytes) : null;
                };

                (async () => {
                    const hashBytes = parseHashBytes({{encodedHashBytes}});
                    const options = hashBytes
                        ? { serverCertificateHashes: [{ algorithm: "sha-256", value: hashBytes.buffer }] }
                        : undefined;
                    const transport = new WebTransport({{encodedUrl}}, options);
                    await transport.ready;
                    window.__wmTransport = transport;
                    window.__wmDatagramWriter = transport.datagrams.writable.getWriter();

                    const reader = transport.datagrams.readable.getReader();
                    while (true) {
                        const result = await reader.read();
                        if (result.done) break;
                        window.__wmSnapshots.push(encodeBase64(result.value));
                    }
                })().catch((error) => {
                    window.__wmTransportError = String(error);
                    console.error(error);
                });
            })();
            """, true);
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

        while (_webSocketPeer.GetAvailablePacketCount() > 0)
        {
            ReceiveSnapshot(_webSocketPeer.GetPacket());
        }
    }

    private static uint GenerateEntityId()
    {
        var timeBits = (uint)Time.GetTicksMsec();
        var randomBits = (uint)GD.Randi();
        var entityId = timeBits ^ randomBits;
        return entityId == 0 ? 1 : entityId;
    }

    private void DrainSnapshots()
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
