using Godot;

namespace WeaponsMastersClient.Game;

/// <summary>
/// Camada de movimento do CharacterBody3D.
/// Toda lógica de física passa delta explícito para garantir reprodução
/// determinística durante o replay de inputs na reconciliação.
/// </summary>
public partial class PlayerController : CharacterBody3D
{
    private const float MovementSpeed = 5.0f;

    /// <summary>
    /// Aplica um passo de movimento preditivo com delta explícito.
    /// O delta deve ser o tick do servidor (1/30s), não o delta do frame do Godot,
    /// para que o replay de inputs produza a mesma trajetória que o servidor simulou.
    /// </summary>
    public void ApplyPredictedMove(Vector2 direction, float tickDelta)
    {
        if (direction.LengthSquared() <= 0.0f)
        {
            return;
        }

        var normalizedDirection = direction.Normalized();
        var displacement = new Vector3(normalizedDirection.X, 0.0f, normalizedDirection.Y)
            * MovementSpeed
            * tickDelta;

        MoveAndCollide(displacement);

        Rotation = new Vector3(
            0.0f,
            Mathf.Atan2(normalizedDirection.X, normalizedDirection.Y),
            0.0f
        );
    }

    public void ApplyAuthoritativePosition(Vector2 position)
    {
        GlobalPosition = new Vector3(position.X, GlobalPosition.Y, position.Y);
    }

    public void ApplyAuthoritativeRotation(float rotation)
    {
        Rotation = new Vector3(0.0f, -rotation + Mathf.Pi / 2.0f, 0.0f);
    }
}
