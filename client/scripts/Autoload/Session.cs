using Godot;
using Wm;

namespace WeaponsMastersClient.Autoload;

/// <summary>
/// Singleton (AutoLoad) que sobrevive a mudanças de cena.
/// Armazena o JWT e o CharacterData devolvidos pela LoginResponse.
///
/// Regra: o token nunca é logado. O CharacterData é usado para
/// inicializar o HUD e informar o servidor do character_id real.
/// </summary>
public partial class Session : Node
{
    public static Session Instance { get; private set; } = null!;

    /// JWT recebido na LoginResponse. Enviado no header de reconexão (Step 4).
    public string Token { get; private set; } = "";

    /// Dados do personagem carregados do PostgreSQL no login.
    /// Null se ainda não logado ou se o servidor não enviou CharacterData.
    public CharacterData? Character { get; private set; }

    /// true se o jogador está autenticado e CharacterData foi recebido.
    public bool IsLoggedIn => Character is not null;

    public override void _Ready()
    {
        Instance = this;
    }

    /// Chamado por LoginScreen ao receber LoginResponse com sucesso.
    public void SetLoginData(string token, CharacterData? character)
    {
        Token = token;
        Character = character;
    }

    /// Limpa sessão ao deslogar ou ao retornar à tela de login.
    public void Clear()
    {
        Token = "";
        Character = null;
    }
}
