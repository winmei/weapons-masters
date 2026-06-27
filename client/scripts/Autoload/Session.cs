#nullable enable
using Godot;
using Wm;

namespace WeaponsMastersClient.Autoload;

/// <summary>
/// Singleton (AutoLoad) que sobrevive a mudanças de cena.
/// Armazena JWT, refresh token e CharacterData devolvidos pela LoginResponse.
///
/// Regra: tokens nunca são logados. O CharacterData inicializa HUD e character_id.
/// </summary>
public partial class Session : Node
{
    public static Session Instance { get; private set; } = null!;

    /// JWT de curta duração (15 min). Enviado no GameAuthPacket.
    public string Token { get; private set; } = "";

    /// Refresh token rotativo (7 dias). Usado no ReAuth após handoff de rede.
    public string RefreshToken { get; private set; } = "";

    public CharacterData? Character { get; private set; }

    public bool IsLoggedIn => Character is not null;

    public override void _Ready()
    {
        Instance = this;
    }

    public void SetLoginData(string token, string refreshToken, CharacterData? character)
    {
        Token = token;
        RefreshToken = refreshToken;
        Character = character;
    }

    /// Atualiza tokens após rotação (refresh endpoint ou SessionReAuthResult).
    public void UpdateTokens(string accessToken, string refreshToken)
    {
        Token = accessToken;
        RefreshToken = refreshToken;
    }

    public void Clear()
    {
        Token = "";
        RefreshToken = "";
        Character = null;
    }
}
