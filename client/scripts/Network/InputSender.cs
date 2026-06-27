#nullable enable
using Godot;
using Wm;
using WmInputType = Wm.InputType;

namespace WeaponsMastersClient.Network;

/// <summary>
/// Captura input do jogador e empacota em PlayerInput para envio ao servidor.
/// entity_id foi removido do proto — o servidor atribui a identidade.
/// </summary>
public partial class InputSender : Node
{
    private uint _sequence;
    private Vector2 _lastNonZeroDirection = Vector2.Up;

    public Vector2 CurrentDirection => _lastNonZeroDirection;

    public PlayerInput CaptureMovementInput()
    {
        var rawDirection = Input.GetVector(
            "move_left", "move_right", "move_forward", "move_backward"
        );

        var isMoving = rawDirection.LengthSquared() > 0.0f;
        if (isMoving)
        {
            _lastNonZeroDirection = rawDirection.Normalized();
        }

        return new PlayerInput
        {
            Sequence  = ++_sequence,
            InputType = isMoving ? WmInputType.Move : WmInputType.Stop,
            Direction = new Vec2 { X = rawDirection.X, Y = rawDirection.Y },
        };
    }

    public PlayerInput CaptureDodgeInput()
    {
        return new PlayerInput
        {
            Sequence  = ++_sequence,
            InputType = WmInputType.Dodge,
            Direction = new Vec2 { X = _lastNonZeroDirection.X, Y = _lastNonZeroDirection.Y },
            Dodge = new DodgeInput
            {
                Direction = new Vec2 { X = _lastNonZeroDirection.X, Y = _lastNonZeroDirection.Y }
            },
        };
    }

    public PlayerInput CaptureSkillInput(uint skillId, uint targetEntityId)
    {
        return new PlayerInput
        {
            Sequence  = ++_sequence,
            InputType = WmInputType.Skill,
            SkillUse  = new SkillUse { SkillId = skillId, TargetEntityId = targetEntityId },
        };
    }
}
