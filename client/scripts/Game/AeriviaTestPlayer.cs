#nullable enable
using System;
using Godot;

namespace WeaponsMastersClient.Game;

/// <summary>Movimentacao local provisoria para a cena offline de Aerivia.</summary>
public partial class AeriviaTestPlayer : CharacterBody3D
{
    [Export] public float MovementSpeed { get; set; } = 7.0f;
    [Export] public float Acceleration { get; set; } = 24.0f;
    [Export] public float JumpVelocity { get; set; } = 7.0f;
    [Export] public float MouseSensitivity { get; set; } = 0.003f;

    private Node3D _cameraPivot = null!;
    private Node3D _visual = null!;
    private AnimationPlayer _animationPlayer = null!;
    private string _currentAnimation = string.Empty;
    private float _gravity;

    public override void _Ready()
    {
        _cameraPivot = GetNode<Node3D>("CameraPivot");
        _visual = GetNode<Node3D>("Visual");
        _animationPlayer = FindAnimationPlayer(_visual)
            ?? throw new InvalidOperationException("AnimationPlayer do personagem nao encontrado.");
        _gravity = (float)ProjectSettings.GetSetting("physics/3d/default_gravity", 9.8).AsDouble();
        ConfigureAnimations();

        if (DisplayServer.GetName() != "headless")
        {
            Input.MouseMode = Input.MouseModeEnum.Captured;
        }
    }

    public override void _UnhandledInput(InputEvent inputEvent)
    {
        if (inputEvent is InputEventMouseMotion mouseMotion
            && Input.MouseMode == Input.MouseModeEnum.Captured)
        {
            _cameraPivot.RotateY(-mouseMotion.Relative.X * MouseSensitivity);
            var pivotRotation = _cameraPivot.Rotation;
            pivotRotation.X = Mathf.Clamp(
                pivotRotation.X - mouseMotion.Relative.Y * MouseSensitivity,
                Mathf.DegToRad(-60.0f),
                Mathf.DegToRad(25.0f)
            );
            _cameraPivot.Rotation = pivotRotation;
        }
        else if (inputEvent is InputEventKey keyEvent
                 && keyEvent.Pressed
                 && keyEvent.Keycode == Key.Escape)
        {
            Input.MouseMode = Input.MouseModeEnum.Visible;
        }
        else if (inputEvent is InputEventMouseButton mouseButton
                 && mouseButton.Pressed
                 && mouseButton.ButtonIndex == MouseButton.Left)
        {
            Input.MouseMode = Input.MouseModeEnum.Captured;
        }
    }

    public override void _PhysicsProcess(double delta)
    {
        float frameDelta = (float)delta;
        var velocity = Velocity;
        if (!IsOnFloor())
        {
            velocity.Y -= _gravity * frameDelta;
        }
        if (Input.IsActionJustPressed("dodge") && IsOnFloor())
        {
            velocity.Y = JumpVelocity;
        }

        Vector2 inputVector = Input.GetVector(
            "move_left",
            "move_right",
            "move_forward",
            "move_backward"
        );
        Vector3 cameraForward = -_cameraPivot.GlobalTransform.Basis.Z;
        cameraForward.Y = 0.0f;
        cameraForward = cameraForward.Normalized();
        Vector3 cameraRight = _cameraPivot.GlobalTransform.Basis.X;
        cameraRight.Y = 0.0f;
        cameraRight = cameraRight.Normalized();
        Vector3 direction = (cameraRight * inputVector.X - cameraForward * inputVector.Y).Normalized();

        if (!direction.IsZeroApprox())
        {
            velocity.X = Mathf.MoveToward(velocity.X, direction.X * MovementSpeed, Acceleration * frameDelta);
            velocity.Z = Mathf.MoveToward(velocity.Z, direction.Z * MovementSpeed, Acceleration * frameDelta);
            _visual.LookAt(_visual.GlobalPosition + direction, Vector3.Up, true);
        }
        else
        {
            velocity.X = Mathf.MoveToward(velocity.X, 0.0f, Acceleration * frameDelta);
            velocity.Z = Mathf.MoveToward(velocity.Z, 0.0f, Acceleration * frameDelta);
        }

        Velocity = velocity;
        MoveAndSlide();
        UpdateLocomotionAnimation();
    }

    private void ConfigureAnimations()
    {
        foreach (var animationName in new StringName[] { "Idle", "Walk", "Run" })
        {
            if (_animationPlayer.HasAnimation(animationName))
            {
                _animationPlayer.GetAnimation(animationName).LoopMode = Animation.LoopModeEnum.Linear;
            }
        }

        PlayAnimation("Idle");
        GD.Print("AERIVIA_PLAYER_CHARACTER_READY");
        GD.Print(
            $"AERIVIA_PLAYER_CHARACTER_ANIMATIONS={string.Join('|', _animationPlayer.GetAnimationList())}"
        );
    }

    private void UpdateLocomotionAnimation()
    {
        float horizontalSpeed = new Vector2(Velocity.X, Velocity.Z).Length();
        string nextAnimation = horizontalSpeed switch
        {
            < 0.1f => "Idle",
            var speed when speed < MovementSpeed * 0.7f => "Walk",
            _ => "Run",
        };
        PlayAnimation(nextAnimation);
    }

    private void PlayAnimation(string animationName)
    {
        if (_currentAnimation == animationName || !_animationPlayer.HasAnimation(animationName))
        {
            return;
        }

        _animationPlayer.Play(animationName, customBlend: 0.15);
        _currentAnimation = animationName;
    }

    private static AnimationPlayer? FindAnimationPlayer(Node node)
    {
        if (node is AnimationPlayer animationPlayer)
        {
            return animationPlayer;
        }

        foreach (var child in node.GetChildren())
        {
            var result = FindAnimationPlayer(child);
            if (result is not null)
            {
                return result;
            }
        }

        return null;
    }
}
