#nullable enable
using Godot;
using Google.Protobuf;
using Wm;

namespace WeaponsMastersClient.UI;

/// <summary>
/// Tela de login e registro.
/// Login e registro usam mensagens proto distintas (LoginRequest / RegisterRequest)
/// em vez do prefixo "REGISTER:" na senha — elimina parsing frágil de string
/// e garante que a senha não seja alterada antes de chegar ao servidor.
/// </summary>
public partial class LoginScreen : Control
{
    private const string ServerWebSocketUrl = "ws://127.0.0.1:8081";
    private const string MainScenePath = "res://scenes/Main.tscn";

    private VBoxContainer? _loginPanel;
    private VBoxContainer? _registerPanel;

    private LineEdit? _loginUsernameField;
    private LineEdit? _loginPasswordField;
    private Button? _loginButton;
    private Button? _goToRegisterButton;
    private Label? _loginStatusLabel;

    private LineEdit? _registerUsernameField;
    private LineEdit? _registerPasswordField;
    private Button? _registerButton;
    private Button? _goToLoginButton;
    private Label? _registerStatusLabel;

    private WebSocketPeer? _ws;
    private bool _waitingForResponse;
    private bool _pendingIsRegister;
    private bool _connectingBeforeSend;
    private string _pendingUsername = "";
    private string _pendingPassword = "";

    public override void _Ready()
    {
        _loginPanel = GetNode<VBoxContainer>("MainLayout/VBox/CenterRow/CenterCol/LoginPanel/Margin/VBox");
        _registerPanel = GetNode<VBoxContainer>("MainLayout/VBox/CenterRow/CenterCol/RegisterPanel/Margin/VBox");

        _loginUsernameField = GetNode<LineEdit>("MainLayout/VBox/CenterRow/CenterCol/LoginPanel/Margin/VBox/UsernameField");
        _loginPasswordField = GetNode<LineEdit>("MainLayout/VBox/CenterRow/CenterCol/LoginPanel/Margin/VBox/PasswordField");
        _loginButton = GetNode<Button>("MainLayout/VBox/CenterRow/CenterCol/LoginPanel/Margin/VBox/LoginButton");
        _goToRegisterButton = GetNode<Button>("MainLayout/VBox/CenterRow/CenterCol/LoginPanel/Margin/VBox/GoToRegisterButton");
        _loginStatusLabel = GetNode<Label>("MainLayout/VBox/CenterRow/CenterCol/LoginPanel/Margin/VBox/StatusLabel");

        _registerUsernameField = GetNode<LineEdit>("MainLayout/VBox/CenterRow/CenterCol/RegisterPanel/Margin/VBox/UsernameField");
        _registerPasswordField = GetNode<LineEdit>("MainLayout/VBox/CenterRow/CenterCol/RegisterPanel/Margin/VBox/PasswordField");
        _registerButton = GetNode<Button>("MainLayout/VBox/CenterRow/CenterCol/RegisterPanel/Margin/VBox/RegisterButton");
        _goToLoginButton = GetNode<Button>("MainLayout/VBox/CenterRow/CenterCol/RegisterPanel/Margin/VBox/GoToLoginButton");
        _registerStatusLabel = GetNode<Label>("MainLayout/VBox/CenterRow/CenterCol/RegisterPanel/Margin/VBox/StatusLabel");

        _loginButton.Pressed += () => BeginAuth(isRegister: false);
        _registerButton.Pressed += () => BeginAuth(isRegister: true);

        _goToRegisterButton.Pressed += () => SwitchPanel(showRegister: true);
        _goToLoginButton.Pressed += () => SwitchPanel(showRegister: false);
        
        SwitchPanel(showRegister: false);
    }
    
    private void SwitchPanel(bool showRegister)
    {
        var loginBase = GetNode<Control>("MainLayout/VBox/CenterRow/CenterCol/LoginPanel");
        var registerBase = GetNode<Control>("MainLayout/VBox/CenterRow/CenterCol/RegisterPanel");
        if (loginBase != null) loginBase.Visible = !showRegister;
        if (registerBase != null) registerBase.Visible = showRegister;
        
        // Clear status labels when switching
        SetStatus("", false, true);
        SetStatus("", false, false);
    }

    public override void _Process(double delta)
    {
        if (_ws is null)
        {
            return;
        }

        _ws.Poll();
        HandleConnectionState();
        DrainResponses();
    }

    private void BeginAuth(bool isRegister)
    {
        var username = isRegister ? _registerUsernameField?.Text.Trim() : _loginUsernameField?.Text.Trim();
        var password = isRegister ? _registerPasswordField?.Text : _loginPasswordField?.Text;

        username ??= "";
        password ??= "";

        if (string.IsNullOrEmpty(username) || string.IsNullOrEmpty(password))
        {
            SetStatus("Preencha username e password.", error: true, isRegister);
            return;
        }

        _pendingIsRegister = isRegister;
        _pendingUsername = username;
        _pendingPassword = password;

        SetStatus(isRegister ? "Registrando..." : "Conectando...", error: false, isRegister);
        OpenWebSocketConnection();
    }

    private void OpenWebSocketConnection()
    {
        _ws = new WebSocketPeer();
        var error = _ws.ConnectToUrl(ServerWebSocketUrl);
        if (error != Error.Ok)
        {
            SetStatus($"Falha ao conectar: {error}", error: true, _pendingIsRegister);
            _ws = null;
            return;
        }

        _connectingBeforeSend = true;
        _waitingForResponse = false;
    }

    private void HandleConnectionState()
    {
        if (!_connectingBeforeSend || _ws is null)
        {
            return;
        }

        var state = _ws.GetReadyState();
        if (state == WebSocketPeer.State.Open)
        {
            GD.Print("WebSocket: Connection Open! Sending Auth Request...");
            _connectingBeforeSend = false;
            _waitingForResponse = true;
            SendAuthRequest();
        }
        else if (state == WebSocketPeer.State.Closed)
        {
            var closeCode = _ws.GetCloseCode();
            var closeReason = _ws.GetCloseReason();
            GD.Print($"WebSocket: Connection Closed while connecting! Code={closeCode}, Reason={closeReason}");
            _connectingBeforeSend = false;
            SetStatus($"Servidor indisponível ({closeCode})", error: true, _pendingIsRegister);
            _ws = null;
        }
    }

    private void SendAuthRequest()
    {
        if (_ws is null)
        {
            return;
        }

        // Auth gateway protocol: first byte is message type discriminator,
        // followed by protobuf payload. This prevents decode ambiguity between
        // LoginRequest and RegisterRequest which share identical field numbers.
        const byte AuthMsgLogin = 0;
        const byte AuthMsgRegister = 1;

        byte[] protoPayload = _pendingIsRegister
            ? new RegisterRequest { Username = _pendingUsername, Password = _pendingPassword }.ToByteArray()
            : new LoginRequest    { Username = _pendingUsername, Password = _pendingPassword }.ToByteArray();

        byte msgType = _pendingIsRegister ? AuthMsgRegister : AuthMsgLogin;
        byte[] payload = new byte[1 + protoPayload.Length];
        payload[0] = msgType;
        protoPayload.CopyTo(payload, 1);

        var err = _ws.PutPacket(payload);
        if (err != Error.Ok)
        {
            GD.PrintErr($"Failed to put packet: {err}");
        }
    }

    private void DrainResponses()
    {
        if (!_waitingForResponse || _ws is null)
        {
            return;
        }

        while (_ws.GetAvailablePacketCount() > 0)
        {
            HandleResponse(_ws.GetPacket());
            if (!_waitingForResponse) return;
        }

        if (_waitingForResponse && _ws.GetReadyState() == WebSocketPeer.State.Closed)
        {
            var closeCode = _ws.GetCloseCode();
            var closeReason = _ws.GetCloseReason();
            GD.Print($"WebSocket: Closed while waiting for response! Code={closeCode}, Reason={closeReason}");
            SetStatus($"Conexão perdida ({closeCode}): {closeReason}", error: true, _pendingIsRegister);
            _waitingForResponse = false;
        }
    }

    private void HandleResponse(byte[] payload)
    {
        _waitingForResponse = false;

        LoginResponse response;
        try
        {
            response = LoginResponse.Parser.ParseFrom(payload);
        }
        catch (InvalidProtocolBufferException ex)
        {
            GD.PushError($"LoginScreen: invalid response payload — {ex.Message}");
            SetStatus("Resposta inválida do servidor.", error: true, _pendingIsRegister);
            return;
        }

        if (response.Success)
        {
            // Store token and character data in the session singleton before
            // switching scenes — never log the token value.
            WeaponsMastersClient.Autoload.Session.Instance?.SetLoginData(
                response.Token,
                response.RefreshToken,
                response.Character
            );
            SetStatus("Login bem-sucedido! Carregando mundo...", error: false, _pendingIsRegister);
            GetTree().ChangeSceneToFile(MainScenePath);
        }
        else
        {
            SetStatus($"Erro: {response.ErrorMessage}", error: true, _pendingIsRegister);
        }
    }

    private void SetStatus(string message, bool error, bool isRegister)
    {
        var targetLabel = isRegister ? _registerStatusLabel : _loginStatusLabel;
        if (targetLabel is null)
        {
            return;
        }
        targetLabel.Text     = message;
        targetLabel.Modulate = error ? Colors.OrangeRed : Colors.White;
    }
}
