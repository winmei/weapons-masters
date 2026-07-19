#nullable enable
using System;
using System.Collections.Generic;
using System.Linq;
using Godot;

namespace WeaponsMastersClient.Game;

/// <summary>
/// Configura exclusivamente a cena offline de teste do blockout de Aerivia.
/// Nao acessa rede, servidor, autoload de sessao ou sistemas de multiplayer.
/// </summary>
public partial class AeriviaTestScene : Node3D
{
    private static readonly string[] WalkablePrefixes =
    {
        "BLK_Nivel_",
        "BLK_Terreno_",
        "BLK_Caminho_",
        "BLK_Praca_",
        "BLK_Ponte_Principal",
        "BLK_Rampa_",
        "BLK_Spawn_Plataforma",
        "BLK_Santuario_Base",
        "BLK_Ruinas_Plataforma_",
    };

    private StandardMaterial3D _stoneMaterial = null!;
    private StandardMaterial3D _terrainMaterial = null!;
    private StandardMaterial3D _foliageMaterial = null!;
    private StandardMaterial3D _trunkMaterial = null!;
    private StandardMaterial3D _waterMaterial = null!;
    private CharacterBody3D _player = null!;
    private int _generatedCollisionCount;
    private int _materialOverrideCount;
    private bool _runtimeValidated;

    public override void _Ready()
    {
        _player = GetNode<CharacterBody3D>("Player");
        CreatePreviewMaterials();

        var mapMeshes = new List<MeshInstance3D>();
        CollectMeshes(GetNode<Node3D>("AeriviaMap"), mapMeshes);
        foreach (var meshInstance in mapMeshes)
        {
            var objectName = meshInstance.Name.ToString();
            meshInstance.MaterialOverride = MaterialFor(objectName);
            _materialOverrideCount++;
            if (NeedsWalkableCollision(objectName))
            {
                meshInstance.CreateTrimeshCollision();
                _generatedCollisionCount++;
            }
        }

        GD.Print($"AERIVIA_TEST_MAP_MESHES={mapMeshes.Count}");
        GD.Print($"AERIVIA_TEST_GENERATED_COLLISIONS={_generatedCollisionCount}");
        GD.Print($"AERIVIA_TEST_PLAYER_POSITION={_player.GlobalPosition}");
    }

    public override void _PhysicsProcess(double delta)
    {
        if (_runtimeValidated)
        {
            return;
        }

        _runtimeValidated = true;
        ValidateRuntime();
    }

    private void ValidateRuntime()
    {
        var expectedCamera = GetNode<Camera3D>("Player/CameraPivot/SpringArm3D/Camera3D");
        var playerShape = GetNode<CollisionShape3D>("Player/CollisionShape3D").Shape as CapsuleShape3D;
        bool playerScaleOk = _player.Scale.IsEqualApprox(Vector3.One);
        bool playerHeightOk = playerShape is not null && Mathf.IsEqualApprox(playerShape.Height, 1.8f);
        bool cameraOk = GetViewport().GetCamera3D() == expectedCamera;
        bool collisionOk = _generatedCollisionCount > 0;
        bool materialsOk = _materialOverrideCount > 0;

        GD.Print($"AERIVIA_TEST_PLAYER_SCALE_OK={playerScaleOk}");
        GD.Print($"AERIVIA_TEST_PLAYER_HEIGHT_OK={playerHeightOk}");
        GD.Print($"AERIVIA_TEST_CAMERA_OK={cameraOk}");
        GD.Print($"AERIVIA_TEST_COLLISION_OK={collisionOk}");
        GD.Print($"AERIVIA_TEST_MATERIAL_OVERRIDES={_materialOverrideCount}");
        GD.Print($"AERIVIA_TEST_MATERIALS_OK={materialsOk}");
        GD.Print("AERIVIA_TEST_READY");

        if (OS.GetCmdlineUserArgs().Contains("--aerivia-validation"))
        {
            GetTree().Quit(
                playerScaleOk && playerHeightOk && cameraOk && collisionOk && materialsOk ? 0 : 1
            );
        }
    }

    private static void CollectMeshes(Node node, ICollection<MeshInstance3D> output)
    {
        if (node is MeshInstance3D meshInstance)
        {
            output.Add(meshInstance);
        }

        foreach (var child in node.GetChildren())
        {
            CollectMeshes(child, output);
        }
    }

    private static bool NeedsWalkableCollision(string objectName) =>
        WalkablePrefixes.Any(prefix => objectName.StartsWith(prefix, StringComparison.Ordinal));

    private void CreatePreviewMaterials()
    {
        _stoneMaterial = MakeMaterial(new Color(0.48f, 0.52f, 0.62f), 0.82f);
        _terrainMaterial = MakeMaterial(new Color(0.28f, 0.34f, 0.25f), 0.95f);
        _foliageMaterial = MakeMaterial(new Color(0.18f, 0.43f, 0.22f), 0.9f);
        _trunkMaterial = MakeMaterial(new Color(0.31f, 0.20f, 0.12f), 1.0f);
        _waterMaterial = MakeMaterial(new Color(0.08f, 0.38f, 0.58f), 0.25f, 0.12f);
    }

    private static StandardMaterial3D MakeMaterial(Color color, float roughness, float metallic = 0.0f) =>
        new()
        {
            AlbedoColor = color,
            Roughness = roughness,
            Metallic = metallic,
        };

    private StandardMaterial3D MaterialFor(string objectName)
    {
        if (objectName.StartsWith("BLK_Agua_", StringComparison.Ordinal))
        {
            return _waterMaterial;
        }
        if (objectName.StartsWith("BLK_Nivel_", StringComparison.Ordinal)
            || objectName.StartsWith("BLK_Terreno_", StringComparison.Ordinal))
        {
            return _terrainMaterial;
        }
        if (objectName.Contains("Copa", StringComparison.Ordinal)
            || objectName.Contains("Folhas", StringComparison.Ordinal)
            || objectName.Contains("Vegetacao", StringComparison.Ordinal))
        {
            return _foliageMaterial;
        }
        if (objectName.Contains("Tronco", StringComparison.Ordinal))
        {
            return _trunkMaterial;
        }
        return _stoneMaterial;
    }
}
