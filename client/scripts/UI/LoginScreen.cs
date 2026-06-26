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

    private LineEdit? _usernameField;
    private LineEdit? _passwordField;
    private Button? _loginButton;
    private Button? _registerButton;
    private Label? _statusLabel;

    private WebSocketPeer? _ws;
    private bool _waitingForResponse;
    private bool _pendingIsRegister;
    private bool _connectingBeforeSend;
    private string _pendingUsername = "";
    private string _pendingPassword = "";

    public override void _Ready()
    {
        _usernameField = GetNode<LineEdit>("VBox/UsernameField");
        _passwordField = GetNode<LineEdit>("VBox/PasswordField");
        _loginButton   = GetNode<Button>("VBox/LoginButton");
        _registerButton = GetNode<Button>("VBox/RegisterButton");
        _statusLabel   = GetNode<Label>("VBox/StatusLabel");

        _loginButton.Pressed   += () => BeginAuth(isRegister: false);
        _registerButton.Pressed += () => BeginAuth(isRegister: true);
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
        var username = _usernameField?.Text.Trim() ?? "";
        var password = _passwordField?.Text ?? "";

        if (string.IsNullOrEmpty(username) || string.IsNullOrEmpty(password))
        {
            SetStatus("Preencha username e password.", error: true);
            return;
        }

        _pendingIsRegister = isRegister;
        _pendingUsername = username;
        _pendingPassword = password;

        SetStatus(isRegister ? "Registrando..." : "Conectando...");
        OpenWebSocketConnection();
    }

    private void OpenWebSocketConnection()
    {
        _ws = new WebSocketPeer();
        var error = _ws.ConnectToUrl(ServerWebSocketUrl);
        if (error != Error.Ok)
        {
            SetStatus($"Falha ao conectar: {error}", error: true);
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
            _connectingBeforeSend = false;
            _waitingForResponse = true;
            SendAuthRequest();
        }
        else if (state == WebSocketPeer.State.Closed)
        {
            _connectingBeforeSend = false;
            SetStatus("Servidor indisponível.", error: true);
            _ws = null;
        }
    }

    private void SendAuthRequest()
    {
        if (_ws is null)
        {
            return;
        }

        // Mensagens proto distintas evitam o prefixo "REGISTER:" na senha
        byte[] payload = _pendingIsRegister
            ? new RegisterRequest { Username = _pendingUsername, Password = _pendingPassword }.ToByteArray()
            : new LoginRequest    { Username = _pendingUsername, Password = _pendingPassword }.ToByteArray();

        _ws.PutPacket(payload);
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
        }

        if (_ws.GetReadyState() == WebSocketPeer.State.Closed)
        {
            SetStatus("Conexão perdida com o servidor.", error: true);
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
            SetStatus("Resposta inválida do servidor.", error: true);
            return;
        }

        if (response.Success)
        {
            // Store token and character data in the session singleton before
            // switching scenes — never log the token value.
            WeaponsMastersClient.Autoload.Session.Instance?.SetLoginData(
                response.Token,
                response.Character
            );
            SetStatus("Login bem-sucedido! Carregando mundo...");
            GetTree().ChangeSceneToFile(MainScenePath);
        }
        else
        {
            SetStatus($"Erro: {response.ErrorMessage}", error: true);
        }
    }

    private void SetStatus(string message, bool error = false)
    {
        if (_statusLabel is null)
        {
            return;
        }
        _statusLabel.Text     = message;
        _statusLabel.Modulate = error ? Colors.OrangeRed : Colors.White;
    }
}
