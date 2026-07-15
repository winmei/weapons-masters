#nullable enable
using System;
using System.Collections.Generic;
using Godot;
using WeaponsMastersClient.Autoload;
using WeaponsMastersClient.Game;
using WeaponsMastersClient.UI;
using WeaponsMastersClient.Prediction;
using Wm;
using WmEntityAction = Wm.EntityAction;

namespace WeaponsMastersClient.Network;

/// <summary>
/// Aplica snapshots do servidor à cena: reconcilia o jogador local,
/// interpola jogadores remotos com fator frame-rate independent e exibe
/// eventos de combate como floating text.
/// </summary>
public partial class PacketHandler : Node
{
    private const float FloatingTextStartHeight = 2.1f;
    private const float FloatingTextEndHeight = 3.0f;
    private const double FloatingTextDurationSeconds = 0.8;

    // Fator base para interpolação exponencial frame-rate independent.
    // Equivale a ~90% de convergência em 100ms a qualquer frame rate.
    private const float RemotePlayerLerpHalfLife = 10.0f;

    [Export] public NodePath LocalPlayerPath { get; set; } = new("");
    [Export] public NodePath LocalHpLabelPath { get; set; } = new("");
    [Export] public NodePath LocalHpFramePath { get; set; } = new("");
    [Export] public NodePath LocalHpBarPath { get; set; } = new("");
    [Export] public NodePath RemotePlayersRootPath { get; set; } = new("");
    [Export] public NodePath TargetFramePath { get; set; } = new("");
    [Export] public NodePath TargetHpBarPath { get; set; } = new("");
    [Export] public NodePath CharacterInfoLabelPath { get; set; } = new("");
    [Export] public NodePath XpLabelPath { get; set; } = new("");
    [Export] public NodePath XpBarPath { get; set; } = new("");
    [Export] public NodePath InventoryPanelPath { get; set; } = new("");
    [Export] public uint LocalEntityId { get; set; }

    private readonly Dictionary<uint, RemotePlayerView> _remotePlayers = new();
    private readonly Dictionary<uint, RemotePlayerView> _mobViews = new();
    private ClientPrediction? _localPlayer;
    private Label3D? _localHpLabel;
    private Label? _localHpFrame;
    private ProgressBar? _localHpBar;
    private Node3D? _remotePlayersRoot;
    private Label? _targetFrame;
    private ProgressBar? _targetHpBar;
    private uint _selectedTargetEntityId;
    private WorldEntryService? _worldEntry;

    // Último delta de frame disponível para uso em sistemas que não recebem delta diretamente
    private float _lastFrameDelta = 1.0f / 60.0f;

    // Character info populated from Session on scene load, updated by LevelUpEvents.
    private Label? _characterInfoLabel;
    private Label? _xpLabel;
    private ProgressBar? _xpBar;
    private InventoryPanel? _inventoryPanel;
    private string _characterName = "";
    private int _currentLevel = 1;
    private long _currentExperience;

    public uint SelectedTargetEntityId => _selectedTargetEntityId;

    public override void _Ready()
    {
        _localPlayer       = GetNodeOrNull<ClientPrediction>(LocalPlayerPath);
        _localHpLabel      = GetNodeOrNull<Label3D>(LocalHpLabelPath);
        _localHpFrame      = GetNodeOrNull<Label>(LocalHpFramePath);
        _localHpBar        = GetNodeOrNull<ProgressBar>(LocalHpBarPath);
        _remotePlayersRoot = GetNodeOrNull<Node3D>(RemotePlayersRootPath);
        _targetFrame       = GetNodeOrNull<Label>(TargetFramePath);
        _targetHpBar       = GetNodeOrNull<ProgressBar>(TargetHpBarPath);
        _characterInfoLabel = GetNodeOrNull<Label>(CharacterInfoLabelPath);
        _xpLabel           = GetNodeOrNull<Label>(XpLabelPath);
        _xpBar             = GetNodeOrNull<ProgressBar>(XpBarPath);
        _inventoryPanel    = GetNodeOrNull<InventoryPanel>(InventoryPanelPath);
        InitializeHudFromSession();
    }

    public override void _Process(double delta)
    {
        _lastFrameDelta = (float)delta;
    }

    public void SetLocalEntityId(uint entityId)
    {
        LocalEntityId = entityId;
    }

    public void ConfigureWorldEntryService(WorldEntryService worldEntry)
    {
        _worldEntry = worldEntry ?? throw new ArgumentNullException(nameof(worldEntry));
    }

    public void SelectNextTarget()
    {
        if (_remotePlayers.Count == 0 && _mobViews.Count == 0)
        {
            _selectedTargetEntityId = 0;
            UpdateTargetIndicators();
            return;
        }

        // Merge players + live mobs into one sorted list of targetable IDs
        var sortedIds = new List<uint>(_remotePlayers.Keys);
        foreach (var kv in _mobViews)
        {
            if (kv.Value.Root.Visible) sortedIds.Add(kv.Key);
        }
        sortedIds.Sort();
        if (sortedIds.Count == 0)
        {
            _selectedTargetEntityId = 0;
            UpdateTargetIndicators();
            return;
        }
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
            if (_selectedTargetEntityId == LocalEntityId)
            {
                _selectedTargetEntityId = 0;
            }
        }

        foreach (var entity in snapshot.Entities)
        {
            if (entity.Position is null)
            {
                continue;
            }

            if (entity.EntityId == LocalEntityId)
            {
                ReconcileLocalPlayer(entity);
                if (_worldEntry is not null && !_worldEntry.IsReady)
                {
                    _worldEntry.ConfirmSpawn();
                }
            }
            else
            {
                ApplyRemoteEntity(entity);
            }
        }

        // Render mob entities (same shape as player entities)
        foreach (var mob in snapshot.MobEntities)
        {
            if (mob.Position is null) continue;
            ApplyMobEntity(mob);
        }

        foreach (var combatEvent in snapshot.CombatEvents)
        {
            ApplyCombatEvent(combatEvent);
        }

        // Level-up feedback
        foreach (var levelUp in snapshot.LevelUpEvents)
        {
            if (levelUp.EntityId == LocalEntityId)
            {
                _currentLevel = levelUp.NewLevel;
                _currentExperience = levelUp.NewExperience;
                UpdateCharacterInfoLabel();
                UpdateXpDisplay();
                ShowFloatingTextLocal($"LEVEL UP! {levelUp.NewLevel}", Colors.Gold);
                GD.Print($"[XP] Level up to {levelUp.NewLevel}! XP: {levelUp.NewExperience}");
            }
        }

        // Loot feedback
        foreach (var loot in snapshot.LootDrops)
        {
            if (loot.EntityId == LocalEntityId && loot.Item is not null)
            {
                ShowFloatingTextLocal($"+{loot.Item.ItemName}", Colors.LightGreen);
                GD.Print($"[LOOT] Got: {loot.Item.ItemName}");
                _inventoryPanel?.OnLootDrop(loot.Slot, loot.Item.ItemName, loot.Item.Quantity);
            }
        }

        UpdateTargetFrame();
    }

    private void ReconcileLocalPlayer(EntityState entity)
    {
        _localPlayer!.Reconcile(
            entity.LastProcessedInput,
            entity.Position!,
            entity.Rotation,
            entity.Hp,
            entity.MaxHp
        );
        UpdateLocalHp(entity.Hp, entity.MaxHp);
    }

    private void ApplyRemoteEntity(EntityState entity)
    {
        if (_remotePlayersRoot is null || entity.Position is null)
        {
            return;
        }

        var created = false;
        if (!_remotePlayers.TryGetValue(entity.EntityId, out var remotePlayer))
        {
            remotePlayer = CreateRemotePlayer(entity.EntityId);
            _remotePlayers[entity.EntityId] = remotePlayer;
            _remotePlayersRoot.AddChild(remotePlayer.Root);
            created = true;
        }

        var targetPosition = new Vector3(entity.Position.X, 0.0f, entity.Position.Y);

        // Interpolação frame-rate independent: alpha = 1 - 0.1^(delta * halfLife)
        // Garante a mesma suavidade a 30fps e a 144fps.
        var lerpAlpha = 1.0f - MathF.Pow(0.1f, _lastFrameDelta * RemotePlayerLerpHalfLife);
        remotePlayer.Root.GlobalPosition = created
            ? targetPosition
            : remotePlayer.Root.GlobalPosition.Lerp(targetPosition, lerpAlpha);
        remotePlayer.Root.Rotation = new Vector3(0.0f, -entity.Rotation + Mathf.Pi / 2.0f, 0.0f);
        remotePlayer.Hp = entity.Hp;
        remotePlayer.MaxHp = Math.Max(entity.MaxHp, 1);
        remotePlayer.HpLabel.Text = $"{remotePlayer.Hp}/{remotePlayer.MaxHp}";
        remotePlayer.SelectionIndicator.Visible = entity.EntityId == _selectedTargetEntityId;
    }

    /// <summary>
    /// Renders a mob entity. Mobs use the same view shape as remote players but
    /// with a distinct red color and no selection indicator logic yet.
    /// </summary>
    private void ApplyMobEntity(EntityState mob)
    {
        if (_remotePlayersRoot is null || mob.Position is null) return;

        if (mob.CurrentAction == EntityAction.Dead)
        {
            // Hide dead mobs
            if (_mobViews.TryGetValue(mob.EntityId, out var deadView))
            {
                deadView.Root.Visible = false;
            }
            return;
        }

        var created = false;
        if (!_mobViews.TryGetValue(mob.EntityId, out var mobView))
        {
            mobView = CreateMobView(mob.EntityId);
            _mobViews[mob.EntityId] = mobView;
            _remotePlayersRoot.AddChild(mobView.Root);
            created = true;
        }

        mobView.Root.Visible = true;
        var lerpAlpha = 1.0f - MathF.Pow(0.1f, _lastFrameDelta * RemotePlayerLerpHalfLife);
        var targetPos = new Vector3(mob.Position.X, 0.0f, mob.Position.Y);
        mobView.Root.GlobalPosition = created
            ? targetPos
            : mobView.Root.GlobalPosition.Lerp(targetPos, lerpAlpha);
        mobView.Hp = mob.Hp;
        mobView.MaxHp = Math.Max(mob.MaxHp, 1);
        mobView.HpLabel.Text = $"{mob.Hp}/{mob.MaxHp}";
        // Allow targeting mobs: include them in the selection indicator
        mobView.SelectionIndicator.Visible = mob.EntityId == _selectedTargetEntityId;
    }

    private void ShowFloatingTextLocal(string text, Color color)
    {
        if (_localPlayer is not null)
        {
            AddFloatingText(_localPlayer, text, color);
        }
    }

    private void ApplyCombatEvent(CombatEvent combatEvent)
    {
        switch (combatEvent.EventCase)
        {
            case CombatEvent.EventOneofCase.Damage:
                ShowFloatingText(
                    combatEvent.Damage.TargetEntityId,
                    $"-{combatEvent.Damage.Damage}",
                    Colors.OrangeRed
                );
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

        if (_remotePlayers.TryGetValue(entityId, out var remotePlayer))
        {
            AddFloatingText(remotePlayer.Root, text, color);
            return;
        }

        if (_mobViews.TryGetValue(entityId, out var mobView))
        {
            AddFloatingText(mobView.Root, text, color);
        }
    }

    private void AddFloatingText(Node3D anchor, string text, Color color)
    {
        var label = new Label3D
        {
            Text = text,
            Modulate = color,
            Billboard = BaseMaterial3D.BillboardModeEnum.Enabled,
            Position = new Vector3(0.0f, FloatingTextStartHeight, 0.0f)
        };
        anchor.AddChild(label);

        var tween = CreateTween();
        tween.TweenProperty(label, "position",   new Vector3(0.0f, FloatingTextEndHeight, 0.0f), FloatingTextDurationSeconds);
        tween.Parallel().TweenProperty(label, "modulate:a", 0.0f, FloatingTextDurationSeconds);
        tween.TweenCallback(Callable.From(label.QueueFree));
    }

    private void UpdateTargetIndicators()
    {
        foreach (var entry in _remotePlayers)
        {
            entry.Value.SelectionIndicator.Visible = entry.Key == _selectedTargetEntityId;
        }
        foreach (var entry in _mobViews)
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

        if (_selectedTargetEntityId == 0)
        {
            _targetFrame.Text = "Target: none";
            if (_targetHpBar is not null) _targetHpBar.Value = 0.0;
            return;
        }

        // Check players first, then mobs
        RemotePlayerView? target = null;
        string targetLabel = "";
        if (_remotePlayers.TryGetValue(_selectedTargetEntityId, out var playerTarget))
        {
            target = playerTarget;
            targetLabel = $"Player {_selectedTargetEntityId}";
        }
        else if (_mobViews.TryGetValue(_selectedTargetEntityId, out var mobTarget))
        {
            target = mobTarget;
            targetLabel = $"Mob {_selectedTargetEntityId}";
        }

        if (target is null)
        {
            _targetFrame.Text = "Target: none";
            if (_targetHpBar is not null) _targetHpBar.Value = 0.0;
            return;
        }

        _targetFrame.Text = $"{targetLabel} HP {target.Hp}/{target.MaxHp}";
        if (_targetHpBar is not null)
        {
            _targetHpBar.MaxValue = target.MaxHp;
            _targetHpBar.Value    = target.Hp;
        }
    }

    /// Populates HUD with the character data stored in Session at scene load.
    /// Safe to call when Session.Character is null (dev/anonymous mode).
    private void InitializeHudFromSession()
    {
        var character = Session.Instance?.Character;
        if (character is null)
        {
            return;
        }

        _characterName     = character.Name;
        _currentLevel      = character.Level;
        _currentExperience = character.Experience;

        UpdateCharacterInfoLabel();
        UpdateXpDisplay();
        UpdateLocalHp(character.Hp, character.MaxHp);

        GD.Print($"[HUD] Session loaded — {character.Name} Lv{character.Level} " +
                 $"XP:{character.Experience} HP:{character.Hp}/{character.MaxHp}");
    }

    private void UpdateCharacterInfoLabel()
    {
        if (_characterInfoLabel is not null)
        {
            _characterInfoLabel.Text = string.IsNullOrEmpty(_characterName)
                ? $"Lv {_currentLevel}"
                : $"{_characterName}  |  Lv {_currentLevel}";
        }
    }

    /// Recomputes and refreshes the XP label and bar.
    /// XP threshold formula matches the server: xp_for_level(N) = N * 100.
    private void UpdateXpDisplay()
    {
        var xpRequired = Math.Max((long)_currentLevel * 100, 1);
        if (_xpLabel is not null)
        {
            _xpLabel.Text = $"XP {_currentExperience}/{xpRequired}";
        }
        if (_xpBar is not null)
        {
            _xpBar.MaxValue = xpRequired;
            _xpBar.Value    = _currentExperience;
        }
    }

    private void UpdateLocalHp(int hp, int maxHp)
    {
        var safeMaxHp = Math.Max(maxHp, 1);
        if (_localHpLabel is not null) _localHpLabel.Text = $"{hp}/{safeMaxHp}";
        if (_localHpFrame is not null) _localHpFrame.Text = $"HP {hp}/{safeMaxHp}";
        if (_localHpBar is not null)
        {
            _localHpBar.MaxValue = safeMaxHp;
            _localHpBar.Value    = hp;
        }
    }

    private static RemotePlayerView CreateRemotePlayer(uint entityId)
    {
        var root = new Node3D { Name = $"RemotePlayer_{entityId}" };

        var mesh = new MeshInstance3D
        {
            Name = "Cube",
            Mesh = new BoxMesh(),
            Position = new Vector3(0.0f, 0.5f, 0.0f)
        };
        mesh.MaterialOverride = new StandardMaterial3D
        {
            AlbedoColor = new Color(1.0f, 0.72f, 0.18f, 1.0f)
        };
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

    /// <summary>
    /// Creates a compact mob blockout, distinct from players without obscuring the arena.
    /// Goblins, Orcs and Trolls share the same shape for now (Step 3 prototype art).
    /// </summary>
    private static RemotePlayerView CreateMobView(uint mobId)
    {
        var root = new Node3D { Name = $"Mob_{mobId}" };

        var mesh = new MeshInstance3D
        {
            Name = "Cube",
            Mesh = new BoxMesh { Size = new Vector3(0.75f, 1.0f, 0.75f) },
            Position = new Vector3(0.0f, 0.5f, 0.0f)
        };
        mesh.MaterialOverride = new StandardMaterial3D
        {
            AlbedoColor = new Color(0.9f, 0.15f, 0.15f, 1.0f) // red = mob
        };
        root.AddChild(mesh);

        var hpLabel = new Label3D
        {
            Name = "HpLabel",
            Text = "?/?",
            Billboard = BaseMaterial3D.BillboardModeEnum.Enabled,
            Position = new Vector3(0.0f, 1.2f, 0.0f)
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
            AlbedoColor = new Color(1.0f, 0.2f, 0.2f, 0.65f) // red ring for mobs
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
