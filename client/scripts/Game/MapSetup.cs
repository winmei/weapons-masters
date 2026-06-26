using Godot;

namespace WeaponsMastersClient.Game;

/// <summary>
/// Gera a geometria do mapa inicial em tempo de execução.
///
/// Responsabilidades:
///   - 4 paredes de contorno alinhadas com ARENA_LIMIT do servidor (8 u).
///   - 8 árvores decorativas (tronco + copa) próximas às bordas da arena.
///
/// Toda a geometria é puramente visual. A colisão de contorno é autoritativa
/// no servidor via resolve_world_collisions (world/src/main.rs).
/// A parede de gameplay (LoS) permanece em Main.tscn como StaticBody3D.
///
/// Otimização de recursos:
///   Cada forma e cor distintas são alocadas UMA ÚNICA VEZ em InitSharedResources()
///   e reutilizadas por referência em todas as instâncias.
///   Resultado: 40 alocações → 7 (4 meshes + 3 materiais).
///   Nós que compartilham o mesmo par (Mesh, Material) permitem que o renderer
///   do Godot agrupe os draw calls via instanced rendering.
/// </summary>
public partial class MapSetup : Node3D
{
    // Deve coincidir com ARENA_LIMIT em server/crates/world/src/main.rs.
    private const float ArenaHalf  = 8.0f;
    private const float WallHeight = 2.0f;
    private const float WallThick  = 0.5f;

    private static readonly (float X, float Z)[] TreePositions =
    {
        (-5.5f, -5.5f), ( 5.5f, -5.5f),
        (-5.5f,  5.5f), ( 5.5f,  5.5f),
        ( 0.0f, -6.5f), ( 0.0f,  6.5f),
        (-6.5f,  0.0f), ( 6.5f,  0.0f),
    };

    // -----------------------------------------------------------------------
    // Recursos compartilhados — um por forma distinta e um por cor distinta.
    //
    // Os campos são nullable porque recursos Godot não podem ser criados fora
    // do ciclo de vida do nó (antes de _Ready). São garantidamente não-nulos
    // após InitSharedResources() e antes de qualquer método de spawn.
    // -----------------------------------------------------------------------

    // Meshes: 2 orientações de parede + 2 partes de árvore = 4 no total
    private BoxMesh? _horizontalWallMesh;  // Norte + Sul  (comprido em X)
    private BoxMesh? _verticalWallMesh;    // Oeste + Leste (comprido em Z)
    private BoxMesh? _trunkMesh;           // todos os 8 troncos
    private BoxMesh? _foliageMesh;         // todas as 8 copas

    // Materiais: 3 cores distintas = 3 no total
    private StandardMaterial3D? _stoneMaterial;    // paredes
    private StandardMaterial3D? _trunkMaterial;    // troncos (marrom escuro)
    private StandardMaterial3D? _foliageMaterial;  // copas (verde floresta)

    // -----------------------------------------------------------------------

    public override void _Ready()
    {
        InitSharedResources();
        SpawnBoundaryWalls();
        SpawnTrees();
    }

    // -----------------------------------------------------------------------
    // Inicialização dos recursos compartilhados
    // -----------------------------------------------------------------------

    /// <summary>
    /// Cria cada mesh e material único exatamente uma vez.
    /// Chamado antes de qualquer método de spawn para garantir que todos os
    /// campos de recurso sejam válidos no momento do uso.
    /// </summary>
    private void InitSharedResources()
    {
        float span = ArenaHalf * 2f;

        // --- meshes ---
        _horizontalWallMesh = new BoxMesh { Size = new Vector3(span,   WallHeight, WallThick) };
        _verticalWallMesh   = new BoxMesh { Size = new Vector3(WallThick, WallHeight, span)   };
        _trunkMesh          = new BoxMesh { Size = new Vector3(0.4f, 1.5f, 0.4f)              };
        _foliageMesh        = new BoxMesh { Size = new Vector3(1.5f, 1.5f, 1.5f)              };

        // --- materiais ---
        _stoneMaterial   = new StandardMaterial3D { AlbedoColor = new Color(0.55f, 0.48f, 0.38f, 1f) };
        _trunkMaterial   = new StandardMaterial3D { AlbedoColor = new Color(0.38f, 0.25f, 0.13f, 1f) };
        _foliageMaterial = new StandardMaterial3D { AlbedoColor = new Color(0.13f, 0.52f, 0.13f, 1f) };
    }

    // -----------------------------------------------------------------------
    // Paredes de contorno
    // -----------------------------------------------------------------------

    private void SpawnBoundaryWalls()
    {
        float halfY = WallHeight * 0.5f;

        // Norte (z = -ArenaHalf) e Sul (z = +ArenaHalf): mesma mesh horizontal
        SpawnBox(_horizontalWallMesh!, _stoneMaterial!, new Vector3( 0f, halfY, -ArenaHalf));
        SpawnBox(_horizontalWallMesh!, _stoneMaterial!, new Vector3( 0f, halfY,  ArenaHalf));

        // Oeste (x = -ArenaHalf) e Leste (x = +ArenaHalf): mesma mesh vertical
        SpawnBox(_verticalWallMesh!,   _stoneMaterial!, new Vector3(-ArenaHalf, halfY, 0f));
        SpawnBox(_verticalWallMesh!,   _stoneMaterial!, new Vector3( ArenaHalf, halfY, 0f));
    }

    // -----------------------------------------------------------------------
    // Árvores
    // -----------------------------------------------------------------------

    private void SpawnTrees()
    {
        foreach (var (tx, tz) in TreePositions)
        {
            // Todos os troncos compartilham _trunkMesh + _trunkMaterial
            SpawnBox(_trunkMesh!,   _trunkMaterial!,   new Vector3(tx, 0.75f, tz));
            // Todas as copas compartilham _foliageMesh + _foliageMaterial
            SpawnBox(_foliageMesh!, _foliageMaterial!, new Vector3(tx, 2.25f, tz));
        }
    }

    // -----------------------------------------------------------------------
    // Helper
    // -----------------------------------------------------------------------

    /// <summary>
    /// Instancia um MeshInstance3D usando recursos pré-alocados passados por
    /// referência. Não cria nem aloca recursos — apenas adiciona um nó à cena.
    /// </summary>
    private void SpawnBox(BoxMesh mesh, StandardMaterial3D material, Vector3 position)
    {
        var instance = new MeshInstance3D
        {
            Mesh             = mesh,
            MaterialOverride = material,
            Position         = position,
        };
        AddChild(instance);
    }
}
