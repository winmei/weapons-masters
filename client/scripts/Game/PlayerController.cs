using Godot;

namespace WeaponsMastersClient.Game;

public partial class PlayerController : CharacterBody3D
{
    private const float MovementSpeed = 5.0f;

    public void ApplyPredictedMove(Vector2 direction)
    {
        Velocity = new Vector3(direction.X, 0.0f, direction.Y) * MovementSpeed;
        if (direction.LengthSquared() > 0.0f)
        {
            Rotation = new Vector3(0.0f, Mathf.Atan2(direction.X, direction.Y), 0.0f);
        }

        MoveAndSlide();
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
