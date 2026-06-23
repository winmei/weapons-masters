using Godot;
using Wm;
using WmInputType = Wm.InputType;

namespace WeaponsMastersClient.Network;

public partial class InputSender : Node
{
    [Export] public uint EntityId { get; set; }

    private uint _sequence;
    private Vector2 _lastDirection = Vector2.Up;

    public void SetEntityId(uint entityId)
    {
        EntityId = entityId;
    }

    public Vector2 CurrentDirection => _lastDirection;

    public PlayerInput CaptureMovementInput(uint clientTick)
    {
        var direction = Input.GetVector(
            "move_left",
            "move_right",
            "move_forward",
            "move_backward"
        );

        if (direction.LengthSquared() > 0.0f)
        {
            _lastDirection = direction.Normalized();
        }

        return new PlayerInput
        {
            Sequence = ++_sequence,
            EntityId = EntityId,
            InputType = direction.LengthSquared() > 0.0f ? WmInputType.Move : WmInputType.Stop,
            Direction = new Vec2 { X = direction.X, Y = direction.Y },
            ClientTick = clientTick
        };
    }

    public PlayerInput CaptureDodgeInput(uint clientTick)
    {
        return new PlayerInput
        {
            Sequence = ++_sequence,
            EntityId = EntityId,
            InputType = WmInputType.Dodge,
            Direction = new Vec2 { X = _lastDirection.X, Y = _lastDirection.Y },
            Dodge = new DodgeInput
            {
                Direction = new Vec2 { X = _lastDirection.X, Y = _lastDirection.Y }
            },
            ClientTick = clientTick
        };
    }

    public PlayerInput CaptureSkillInput(uint skillId, uint targetEntityId, uint clientTick)
    {
        return new PlayerInput
        {
            Sequence = ++_sequence,
            EntityId = EntityId,
            InputType = WmInputType.Skill,
            SkillUse = new SkillUse
            {
                SkillId = skillId,
                TargetEntityId = targetEntityId
            },
            ClientTick = clientTick
        };
    }
}
