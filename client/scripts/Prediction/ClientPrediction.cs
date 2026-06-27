#nullable enable
using System.Collections.Generic;
using Godot;
using WeaponsMastersClient.Game;
using Wm;
using WmInputType = Wm.InputType;

namespace WeaponsMastersClient.Prediction;

/// <summary>
/// Aplica client-side prediction e reconcilia com os snapshots autoritativos do servidor.
/// </summary>
public partial class ClientPrediction : PlayerController
{
    // Delta fixo igual ao TICK_RATE do servidor (30Hz = ~33ms).
    // Essencial para que o replay de inputs pendentes reproduza exatamente o
    // mesmo deslocamento simulado pelo servidor, evitando rubber-banding.
    private const float ServerTickDelta = 1.0f / 30.0f;

    private readonly List<PlayerInput> _pendingInputs = new();

    public int Hp { get; private set; } = 200;
    public int MaxHp { get; private set; } = 200;

    public void ApplyLocalInput(PlayerInput input)
    {
        if (input.InputType == WmInputType.Move && input.Direction is not null)
        {
            ApplyPredictedMove(
                new Vector2(input.Direction.X, input.Direction.Y),
                ServerTickDelta
            );
        }
        _pendingInputs.Add(input);
    }

    /// <summary>
    /// Dodge local: usa MoveAndCollide com deslocamento fixo para respeitar a
    /// geometria da cena e não teletransportar através de paredes.
    /// </summary>
    public void ApplyLocalDodge(Vector2 direction)
    {
        if (direction.LengthSquared() <= 0.0f)
        {
            return;
        }

        const float DodgeDistance = 3.0f;
        var displacement = direction.Normalized() * DodgeDistance;
        MoveAndCollide(new Vector3(displacement.X, 0.0f, displacement.Y));
    }

    /// <summary>
    /// Reconcilia o estado local com a posição autoritativa do servidor.
    /// Remove inputs já confirmados e reaplicam os pendentes com delta fixo.
    /// </summary>
    public void Reconcile(
        uint lastProcessedInput,
        Vec2 authoritativePosition,
        float authoritativeRotation,
        int hp,
        int maxHp
    )
    {
        _pendingInputs.RemoveAll(input => input.Sequence <= lastProcessedInput);

        ApplyAuthoritativePosition(
            new Vector2(authoritativePosition.X, authoritativePosition.Y)
        );
        ApplyAuthoritativeRotation(authoritativeRotation);

        Hp = hp;
        MaxHp = maxHp;

        if (hp <= 0)
        {
            _pendingInputs.Clear();
            return;
        }

        ReplayPendingInputs();
    }

    private void ReplayPendingInputs()
    {
        foreach (var pendingInput in _pendingInputs)
        {
            if (pendingInput.InputType == WmInputType.Move && pendingInput.Direction is not null)
            {
                ApplyPredictedMove(
                    new Vector2(pendingInput.Direction.X, pendingInput.Direction.Y),
                    ServerTickDelta
                );
            }
        }
    }
}
