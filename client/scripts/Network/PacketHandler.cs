using System;
using System.Collections.Generic;
using Godot;
using WeaponsMastersClient.Prediction;
using Wm;

namespace WeaponsMastersClient.Network;

public partial class PacketHandler : Node
{
    private const float FloatingTextStartHeight = 2.1f;
    private const float FloatingTextEndHeight = 3.0f;
    private const double FloatingTextDurationSeconds = 0.8;

    [Export] public NodePath LocalPlayerPath { get; set; } = new("");
    [Export] public NodePath LocalHpLabelPath { get; set; } = new("");
    [Export] public NodePath LocalHpFramePath { get; set; } = new("");
    [Export] public NodePath LocalHpBarPath { get; set; } = new("");
    [Export] public NodePath RemotePlayersRootPath { get; set; } = new("");
    [Export] public NodePath TargetFramePath { get; set; } = new("");
    [Export] public NodePath TargetHpBarPath { get; set; } = new("");
    [Export] public uint LocalEntityId { get; set; }

    private readonly Dictionary<uint, RemotePlayerView> _remotePlayers = new();
    private ClientPrediction? _localPlayer;
    private Label3D? _localHpLabel;
    private Label? _localHpFrame;
    private ProgressBar? _localHpBar;
    private Node3D? _remotePlayersRoot;
    private Label? _targetFrame;
    private ProgressBar? _targetHpBar;
    private uint _selectedTargetEntityId;

    public uint SelectedTargetEntityId => _selectedTargetEntityId;

    public override void _Ready()
    {
        _localPlayer = GetNodeOrNull<ClientPrediction>(LocalPlayerPath);
        _localHpLabel = GetNodeOrNull<Label3D>(LocalHpLabelPath);
        _localHpFrame = GetNodeOrNull<Label>(LocalHpFramePath);
        _localHpBar = GetNodeOrNull<ProgressBar>(LocalHpBarPath);
        _remotePlayersRoot = GetNodeOrNull<Node3D>(RemotePlayersRootPath);
        _targetFrame = GetNodeOrNull<Label>(TargetFramePath);
        _targetHpBar = GetNodeOrNull<ProgressBar>(TargetHpBarPath);
    }

    public void SetLocalEntityId(uint entityId)
    {
        LocalEntityId = entityId;
    }

    public void SelectNextTarget()
    {
        if (_remotePlayers.Count == 0)
        {
            _selectedTargetEntityId = 0;
            UpdateTargetIndicators();
            return;
        }

        var sortedIds = new List<uint>(_remotePlayers.Keys);
        sortedIds.Sort();
        var currentIndex = sortedIds.IndexOf(_selectedTargetEntityId);
        var nextIndex = (currentIndex + 1) % sortedIds.Count;
        _selectedTargetEntityId = sortedIds[nextIndex];
        UpdateTargetIndicators();
    }

    public void ApplySnapshot(WorldSnapshot snapshot)
    {
        if (_localPlayer is null)
        {
            return;
        }

        if (snapshot.LocalEntityId != 0 && LocalEntityId != snapshot.LocalEntityId)
        {
            LocalEntityId = snapshot.LocalEntityId;
            _selectedTargetEntityId = _selectedTargetEntityId == LocalEntityId ? 0 : _selectedTargetEntityId;
        }

        foreach (var entity in snapshot.Entities)
        {
            if (entity.Position is null)
            {
                continue;
            }

            if (entity.EntityId == LocalEntityId)
            {
                _localPlayer.Reconcile(
                    entity.LastProcessedInput,
                    entity.Position,
                    entity.Rotation,
                    entity.Hp,
                    entity.MaxHp
                );
                UpdateLocalHp(entity.Hp, entity.MaxHp);
            }
            else
            {
                ApplyRemoteEntity(entity);
            }
        }

        foreach (var combatEvent in snapshot.CombatEvents)
        {
            ApplyCombatEvent(combatEvent);
        }

        UpdateTargetFrame();
    }

    private void ApplyRemoteEntity(EntityState entity)
    {
        if (_remotePlayersRoot is null || entity.Position is null)
        {
            return;
        }

        if (!_remotePlayers.TryGetValue(entity.EntityId, out var remotePlayer))
        {
            remotePlayer = CreateRemotePlayer(entity.EntityId);
            _remotePlayers[entity.EntityId] = remotePlayer;
            _remotePlayersRoot.AddChild(remotePlayer.Root);
        }

        var targetPosition = new Vector3(entity.Position.X, 0.0f, entity.Position.Y);
        remotePlayer.Root.GlobalPosition = remotePlayer.Root.GlobalPosition.Lerp(targetPosition, 0.35f);
        remotePlayer.Root.Rotation = new Vector3(0.0f, -entity.Rotation + Mathf.Pi / 2.0f, 0.0f);
        remotePlayer.Hp = entity.Hp;
        remotePlayer.MaxHp = Math.Max(entity.MaxHp, 1);
        remotePlayer.HpLabel.Text = $"{remotePlayer.Hp}/{remotePlayer.MaxHp}";
        remotePlayer.SelectionIndicator.Visible = entity.EntityId == _selectedTargetEntityId;
    }

    private void ApplyCombatEvent(CombatEvent combatEvent)
    {
        switch (combatEvent.EventCase)
        {
            case CombatEvent.EventOneofCase.Damage:
                ShowFloatingText(combatEvent.Damage.TargetEntityId, $"-{combatEvent.Damage.Damage}", Colors.OrangeRed);
                break;
            case CombatEvent.EventOneofCase.Dodge:
                if (combatEvent.Dodge.Success)
                {
                    ShowFloatingText(combatEvent.Dodge.EntityId, "MISS", Colors.LightSkyBlue);
                }
                break;
            case CombatEvent.EventOneofCase.Death:
                ShowFloatingText(combatEvent.Death.EntityId, "DEAD", Colors.Red);
                break;
        }
    }

    private void ShowFloatingText(uint entityId, string text, Color color)
    {
        if (entityId == LocalEntityId && _localPlayer is not null)
        {
            AddFloatingText(_localPlayer, text, color);
            return;
        }

        if (!_remotePlayers.TryGetValue(entityId, out var remotePlayer))
        {
            return;
        }

        AddFloatingText(remotePlayer.Root, text, color);
    }

    private void AddFloatingText(Node3D root, string text, Color color)
    {
        var label = new Label3D
        {
            Text = text,
            Modulate = color,
            Billboard = BaseMaterial3D.BillboardModeEnum.Enabled,
            Position = new Vector3(0.0f, FloatingTextStartHeight, 0.0f)
        };
        root.AddChild(label);

        var tween = CreateTween();
        tween.TweenProperty(label, "position", new Vector3(0.0f, FloatingTextEndHeight, 0.0f), FloatingTextDurationSeconds);
        tween.Parallel().TweenProperty(label, "modulate:a", 0.0f, FloatingTextDurationSeconds);
        tween.TweenCallback(Callable.From(label.QueueFree));
    }

    private void UpdateTargetIndicators()
    {
        foreach (var entry in _remotePlayers)
        {
            entry.Value.SelectionIndicator.Visible = entry.Key == _selectedTargetEntityId;
        }

        UpdateTargetFrame();
    }

    private void UpdateTargetFrame()
    {
        if (_targetFrame is null)
        {
            return;
        }

        if (_selectedTargetEntityId == 0 || !_remotePlayers.TryGetValue(_selectedTargetEntityId, out var target))
        {
            _targetFrame.Text = "Target: none";
            if (_targetHpBar is not null)
            {
                _targetHpBar.Value = 0.0;
            }
            return;
        }

        _targetFrame.Text = $"Target {_selectedTargetEntityId} HP {target.Hp}/{target.MaxHp}";
        if (_targetHpBar is not null)
        {
            _targetHpBar.MaxValue = target.MaxHp;
            _targetHpBar.Value = target.Hp;
        }
    }

    private void UpdateLocalHp(int hp, int maxHp)
    {
        var safeMaxHp = Math.Max(maxHp, 1);
        if (_localHpLabel is not null)
        {
            _localHpLabel.Text = $"{hp}/{safeMaxHp}";
        }

        if (_localHpFrame is not null)
        {
            _localHpFrame.Text = $"HP {hp}/{safeMaxHp}";
        }

        if (_localHpBar is not null)
        {
            _localHpBar.MaxValue = safeMaxHp;
            _localHpBar.Value = hp;
        }
    }

    private static RemotePlayerView CreateRemotePlayer(uint entityId)
    {
        var root = new Node3D
        {
            Name = $"RemotePlayer_{entityId}"
        };

        var mesh = new MeshInstance3D
        {
            Name = "Cube",
            Mesh = new BoxMesh()
        };
        mesh.MaterialOverride = new StandardMaterial3D
        {
            AlbedoColor = new Color(1.0f, 0.72f, 0.18f, 1.0f)
        };
        mesh.Position = new Vector3(0.0f, 0.5f, 0.0f);
        root.AddChild(mesh);

        var hpLabel = new Label3D
        {
            Name = "HpLabel",
            Text = "200/200",
            Billboard = BaseMaterial3D.BillboardModeEnum.Enabled,
            Position = new Vector3(0.0f, 1.4f, 0.0f)
        };
        root.AddChild(hpLabel);

        var indicator = new MeshInstance3D
        {
            Name = "TargetIndicator",
            Mesh = new CylinderMesh { TopRadius = 0.8f, BottomRadius = 0.8f, Height = 0.04f },
            Position = new Vector3(0.0f, 0.03f, 0.0f),
            Visible = false
        };
        indicator.MaterialOverride = new StandardMaterial3D
        {
            AlbedoColor = new Color(0.1f, 1.0f, 0.4f, 0.65f)
        };
        root.AddChild(indicator);

        return new RemotePlayerView(root, hpLabel, indicator);
    }

    private sealed class RemotePlayerView
    {
        public RemotePlayerView(Node3D root, Label3D hpLabel, MeshInstance3D selectionIndicator)
        {
            Root = root;
            HpLabel = hpLabel;
            SelectionIndicator = selectionIndicator;
        }

        public Node3D Root { get; }
        public Label3D HpLabel { get; }
        public MeshInstance3D SelectionIndicator { get; }
        public int Hp { get; set; } = 200;
        public int MaxHp { get; set; } = 200;
    }
}
