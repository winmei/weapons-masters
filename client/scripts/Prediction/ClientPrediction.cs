using System.Collections.Generic;
using Godot;
using WeaponsMastersClient.Game;
using Wm;
using WmInputType = Wm.InputType;

namespace WeaponsMastersClient.Prediction;

public partial class ClientPrediction : PlayerController
{
    private readonly List<PlayerInput> _pendingInputs = new();

    public int Hp { get; private set; } = 200;
    public int MaxHp { get; private set; } = 200;

    public void ApplyLocalInput(PlayerInput input)
    {
        if (input.InputType == WmInputType.Move && input.Direction is not null)
        {
            ApplyPredictedMove(new Vector2(input.Direction.X, input.Direction.Y));
        }

        _pendingInputs.Add(input);
    }

    public void ApplyLocalDodge(Vector2 direction)
    {
        if (direction.LengthSquared() <= 0.0f)
        {
            return;
        }

        ApplyAuthoritativePosition(
            new Vector2(GlobalPosition.X, GlobalPosition.Z) + direction.Normalized() * 3.0f
        );
    }

    public void Reconcile(
        uint lastProcessedInput,
        Vec2 authoritativePosition,
        float authoritativeRotation,
        int hp,
        int maxHp
    )
    {
        _pendingInputs.RemoveAll(input => input.Sequence <= lastProcessedInput);
        ApplyAuthoritativePosition(new Vector2(authoritativePosition.X, authoritativePosition.Y));
        ApplyAuthoritativeRotation(authoritativeRotation);
        Hp = hp;
        MaxHp = maxHp;

        foreach (var pendingInput in _pendingInputs)
        {
            if (pendingInput.InputType == WmInputType.Move && pendingInput.Direction is not null)
            {
                ApplyPredictedMove(new Vector2(pendingInput.Direction.X, pendingInput.Direction.Y));
            }
        }
    }
}
