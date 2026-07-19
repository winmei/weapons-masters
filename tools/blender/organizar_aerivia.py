"""Organiza exclusivamente as colecoes da cena Aerivia Blockout v002.

Uso esperado:
    blender --background Aerivia_Blockout_v002.blend --python organizar_aerivia.py

O arquivo original nunca e salvo por este script. O resultado sempre e gravado em
Aerivia_Blockout_v003_organizado.blend.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any

import bpy


PROJECT_ROOT = Path(__file__).resolve().parents[2]
SOURCE = (PROJECT_ROOT / "3D" / "blender" / "spawn" / "Aerivia_Blockout_v002.blend").resolve()
OUTPUT = (PROJECT_ROOT / "3D" / "blender" / "spawn" / "Aerivia_Blockout_v003_organizado.blend").resolve()
REPORT = (PROJECT_ROOT / "3D" / "blender" / "spawn" / "Aerivia_organizacao_relatorio.txt").resolve()
SCRIPT = Path(__file__).resolve()

MAIN_COLLECTION_NAMES = (
    "00_REFERENCIAS",
    "01_BLOCKOUT",
    "02_TERRENO",
    "03_ARQUITETURA",
    "04_ARVORE",
    "05_AGUA",
    "06_VEGETACAO",
    "07_PROPS",
    "08_COLISAO",
    "09_OPCIONAIS",
)

warnings: list[str] = []


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest().upper()


def rounded_tuple(values: Any) -> tuple[float, ...]:
    return tuple(float(value) for value in values)


def matrix_tuple(matrix: Any) -> tuple[float, ...]:
    return tuple(float(value) for row in matrix for value in row)


def transform_snapshot(obj: bpy.types.Object) -> dict[str, Any]:
    return {
        "location": rounded_tuple(obj.location),
        "rotation_mode": obj.rotation_mode,
        "rotation_euler": rounded_tuple(obj.rotation_euler),
        "rotation_quaternion": rounded_tuple(obj.rotation_quaternion),
        "rotation_axis_angle": rounded_tuple(obj.rotation_axis_angle),
        "scale": rounded_tuple(obj.scale),
        "delta_location": rounded_tuple(obj.delta_location),
        "delta_rotation_euler": rounded_tuple(obj.delta_rotation_euler),
        "delta_rotation_quaternion": rounded_tuple(obj.delta_rotation_quaternion),
        "delta_scale": rounded_tuple(obj.delta_scale),
        "dimensions": rounded_tuple(obj.dimensions),
        "matrix_basis": matrix_tuple(obj.matrix_basis),
        "matrix_parent_inverse": matrix_tuple(obj.matrix_parent_inverse),
        "matrix_world": matrix_tuple(obj.matrix_world),
    }


def values_equal(before: Any, after: Any, tolerance: float = 1e-9) -> bool:
    if isinstance(before, dict) and isinstance(after, dict):
        return before.keys() == after.keys() and all(
            values_equal(before[key], after[key], tolerance) for key in before
        )
    if isinstance(before, tuple) and isinstance(after, tuple):
        return len(before) == len(after) and all(
            values_equal(left, right, tolerance) for left, right in zip(before, after)
        )
    if isinstance(before, float) or isinstance(after, float):
        return abs(float(before) - float(after)) <= tolerance
    return before == after


def starts(name: str, *prefixes: str) -> bool:
    lowered = name.casefold()
    return any(lowered.startswith(prefix.casefold()) for prefix in prefixes)


def collection_names(obj: bpy.types.Object) -> tuple[str, ...]:
    return tuple(sorted(collection.name for collection in obj.users_collection))


loaded_path = Path(bpy.data.filepath).resolve()
if loaded_path != SOURCE:
    raise RuntimeError(
        f"Arquivo carregado inesperado. Esperado: {SOURCE}; carregado: {loaded_path}"
    )
if not SOURCE.is_file():
    raise FileNotFoundError(SOURCE)

source_hash_before = sha256(SOURCE)
objects_before = tuple(bpy.data.objects)
object_names_before = tuple(sorted(obj.name for obj in objects_before))
object_count_before = len(objects_before)
memberships_before = {obj.name: collection_names(obj) for obj in objects_before}
multi_collection_before = {
    name: memberships
    for name, memberships in memberships_before.items()
    if len(memberships) > 1
}
transforms_before = {obj.name: transform_snapshot(obj) for obj in objects_before}
modifiers_before = {
    obj.name: tuple((modifier.name, modifier.type, modifier.as_pointer()) for modifier in obj.modifiers)
    for obj in objects_before
}
constraints_before = {
    obj.name: tuple((constraint.name, constraint.type, constraint.as_pointer()) for constraint in obj.constraints)
    for obj in objects_before
}
protected_links_before = {
    obj.name: (
        obj.parent.as_pointer() if obj.parent else None,
        obj.data.as_pointer() if obj.data else None,
        obj.instance_collection.as_pointer() if obj.instance_collection else None,
        tuple(slot.material.as_pointer() if slot.material else None for slot in obj.material_slots),
        bool(obj.hide_viewport),
        bool(obj.hide_render),
    )
    for obj in objects_before
}


def get_main(name: str) -> bpy.types.Collection:
    collection = bpy.data.collections.get(name)
    if collection is None:
        raise RuntimeError(f"Colecao principal obrigatoria nao encontrada: {name}")
    return collection


main = {name: get_main(name) for name in MAIN_COLLECTION_NAMES}
created: dict[str, bpy.types.Collection] = {}


def create_child(parent: bpy.types.Collection, intended_name: str, path_key: str) -> bpy.types.Collection:
    collection = bpy.data.collections.new(intended_name)
    parent.children.link(collection)
    created[path_key] = collection
    if collection.name != intended_name:
        warnings.append(
            f"O Blender ajustou o nome interno de {path_key} para {collection.name} "
            "porque nomes de datablocks de colecao precisam ser unicos."
        )
    return collection


# Filhas reais, vinculadas aos respectivos pais. A ordem e deliberada: ela torna
# previsiveis os sufixos internos que o Blender aplica a nomes repetidos.
ref_camera = create_child(main["00_REFERENCIAS"], "CAMERA", "00_REFERENCIAS/CAMERA")
ref_gameplay = create_child(main["00_REFERENCIAS"], "GAMEPLAY", "00_REFERENCIAS/GAMEPLAY")
ref_kit = create_child(main["00_REFERENCIAS"], "ENCAIXES_KIT", "00_REFERENCIAS/ENCAIXES_KIT")

block_city = create_child(main["01_BLOCKOUT"], "CIDADE", "01_BLOCKOUT/CIDADE")
block_south = create_child(main["01_BLOCKOUT"], "AREA_SUL", "01_BLOCKOUT/AREA_SUL")
block_forest = create_child(main["01_BLOCKOUT"], "BOSQUE", "01_BLOCKOUT/BOSQUE")
block_spring = create_child(main["01_BLOCKOUT"], "NASCENTE", "01_BLOCKOUT/NASCENTE")
block_ruins = create_child(main["01_BLOCKOUT"], "RUINAS", "01_BLOCKOUT/RUINAS")

terrain_city = create_child(main["02_TERRENO"], "CIDADE", "02_TERRENO/CIDADE")
terrain_ruins = create_child(main["02_TERRENO"], "RUINAS", "02_TERRENO/RUINAS")

arch_structural = create_child(
    main["03_ARQUITETURA"], "KIT_ESTRUTURAL", "03_ARQUITETURA/KIT_ESTRUTURAL"
)
arch_structural_children: dict[str, bpy.types.Collection] = {}
for child_name in (
    "PISOS",
    "COLUNAS",
    "ARCOS",
    "GUARDA_CORPOS",
    "COBERTURAS",
    "ESCADAS",
    "PAREDES",
    "CANTOS",
    "MOLDURAS",
    "PONTES",
    "PRACAS",
):
    key = f"03_ARQUITETURA/KIT_ESTRUTURAL/{child_name}"
    arch_structural_children[child_name] = create_child(arch_structural, child_name, key)

arch_urban = create_child(main["03_ARQUITETURA"], "KIT_URBANO", "03_ARQUITETURA/KIT_URBANO")
arch_urban_children: dict[str, bpy.types.Collection] = {}
for child_name in ("PEDESTAIS", "FONTES", "POSTES", "BANCOS", "VASOS"):
    key = f"03_ARQUITETURA/KIT_URBANO/{child_name}"
    arch_urban_children[child_name] = create_child(arch_urban, child_name, key)

water_map = create_child(main["05_AGUA"], "MAPA", "05_AGUA/MAPA")
water_kit = create_child(main["05_AGUA"], "KIT_MODULAR", "05_AGUA/KIT_MODULAR")
vegetation_map = create_child(main["06_VEGETACAO"], "MAPA", "06_VEGETACAO/MAPA")
vegetation_kit = create_child(main["06_VEGETACAO"], "KIT_MODULAR", "06_VEGETACAO/KIT_MODULAR")
props_urban = create_child(main["07_PROPS"], "KIT_URBANO", "07_PROPS/KIT_URBANO")


def classify(obj: bpy.types.Object) -> bpy.types.Collection | None:
    name = obj.name
    original_collections = set(memberships_before[name])

    if starts(name, "REF_Camera_"):
        return ref_camera
    if starts(
        name,
        "REF_Jogador_",
        "REF_NPC_",
        "REF_Spawn_",
        "REF_Recurso_",
        "REF_Ponto_Retorno_",
    ):
        return ref_gameplay
    if starts(name, "REF_Aerivia_"):
        return ref_kit

    # Regras explicitas de agua e terreno tem prioridade sobre categorias amplas.
    if starts(name, "BLK_Agua_"):
        return water_map
    if starts(name, "BLK_Nivel_", "BLK_Terreno_Saida_Sul"):
        return terrain_city
    if starts(name, "BLK_Terreno_Ruinas_"):
        return terrain_ruins

    # O prompt pede que BLK_ ja existentes em 06_VEGETACAO permaneçam no ramo MAPA.
    if "06_VEGETACAO" in original_collections and starts(name, "BLK_"):
        return vegetation_map

    if starts(
        name,
        "BLK_Praca_",
        "BLK_Santuario_",
        "BLK_Monumento_",
        "BLK_Pavilhao_",
        "BLK_Rampa_",
        "BLK_Ponte_Principal",
        "BLK_Caminho_Ponte_Praca",
        "BLK_Caminho_Praca_",
    ):
        return block_city
    if starts(
        name,
        "BLK_Caminho_Saida_Sul",
        "BLK_Area_Chegada_Sul",
        "BLK_Placa_Bifurcacao_",
        "BLK_Caminho_Bosque_",
        "BLK_Caminho_Margens_",
        "BLK_Caminho_Nascente_",
        "BLK_Area_Bosque_",
        "BLK_Area_Margens_",
        "BLK_Limite_Sul_",
    ):
        return block_south
    if starts(name, "BLK_Bosque_"):
        return block_forest
    if starts(name, "BLK_Nascente_"):
        return block_spring
    if starts(name, "BLK_Ruinas_"):
        return block_ruins

    if starts(name, "MOD_Aerivia_Fonte_Agua"):
        return water_kit
    if starts(
        name,
        "MOD_Aerivia_Arvore_Pequena_",
        "MOD_Aerivia_Arbusto_",
        "MOD_Aerivia_Folhas_",
    ):
        return vegetation_kit
    if starts(name, "MOD_Aerivia_Poste_Cristal", "MOD_Aerivia_Vaso_Substrato"):
        return props_urban

    structural_rules = (
        ("PISOS", "MOD_Aerivia_Piso_"),
        ("COLUNAS", "MOD_Aerivia_Coluna_"),
        ("ARCOS", "MOD_Aerivia_Arco_"),
        ("GUARDA_CORPOS", "MOD_Aerivia_GuardaCorpo_"),
        ("COBERTURAS", "MOD_Aerivia_Cobertura_"),
        ("ESCADAS", "MOD_Aerivia_Escada_"),
        ("PAREDES", "MOD_Aerivia_Parede_"),
        ("CANTOS", "MOD_Aerivia_Canto_"),
        ("MOLDURAS", "MOD_Aerivia_Moldura_"),
        ("PONTES", "MOD_Aerivia_Ponte_"),
        ("PRACAS", "MOD_Aerivia_Praca_"),
    )
    for child_name, prefix in structural_rules:
        if starts(name, prefix):
            return arch_structural_children[child_name]

    urban_rules = (
        ("PEDESTAIS", "MOD_Aerivia_Pedestal_"),
        ("FONTES", "MOD_Aerivia_Fonte_"),
        ("POSTES", "MOD_Aerivia_Poste_"),
        ("BANCOS", "MOD_Aerivia_Banco_"),
        ("VASOS", "MOD_Aerivia_Vaso_"),
    )
    for child_name, prefix in urban_rules:
        if starts(name, prefix):
            return arch_urban_children[child_name]

    # Outros MOD_Aerivia_ decorativos ja identificados pelo artista em 07_PROPS.
    if "07_PROPS" in original_collections and starts(name, "MOD_Aerivia_"):
        return props_urban
    return None


classified: dict[str, bpy.types.Collection] = {}
unclassified: list[str] = []
scene_root = bpy.context.scene.collection

for obj in objects_before:
    target = classify(obj)
    if target is None:
        unclassified.append(obj.name)
        continue
    classified[obj.name] = target
    if target not in obj.users_collection:
        target.objects.link(obj)
    # Vincular primeiro e so depois remover os vinculos organizacionais antigos.
    for previous_collection in tuple(obj.users_collection):
        if previous_collection != target and previous_collection != scene_root:
            previous_collection.objects.unlink(obj)

bpy.context.view_layer.update()

objects_after = tuple(bpy.data.objects)
object_names_after = tuple(sorted(obj.name for obj in objects_after))
object_count_after = len(objects_after)
transforms_changed = sorted(
    obj.name
    for obj in objects_after
    if not values_equal(transforms_before[obj.name], transform_snapshot(obj))
)
modifiers_changed = sorted(
    obj.name
    for obj in objects_after
    if modifiers_before[obj.name]
    != tuple((modifier.name, modifier.type, modifier.as_pointer()) for modifier in obj.modifiers)
)
constraints_changed = sorted(
    obj.name
    for obj in objects_after
    if constraints_before[obj.name]
    != tuple((constraint.name, constraint.type, constraint.as_pointer()) for constraint in obj.constraints)
)
protected_links_changed = sorted(
    obj.name
    for obj in objects_after
    if protected_links_before[obj.name]
    != (
        obj.parent.as_pointer() if obj.parent else None,
        obj.data.as_pointer() if obj.data else None,
        obj.instance_collection.as_pointer() if obj.instance_collection else None,
        tuple(slot.material.as_pointer() if slot.material else None for slot in obj.material_slots),
        bool(obj.hide_viewport),
        bool(obj.hide_render),
    )
)

collection_membership_errors: list[str] = []
for object_name, target in classified.items():
    obj = bpy.data.objects[object_name]
    organizational = [collection for collection in obj.users_collection if collection != scene_root]
    if organizational != [target]:
        collection_membership_errors.append(
            f"{object_name}: esperado {target.name}; encontrado "
            + ", ".join(collection.name for collection in organizational)
        )

if object_count_after != object_count_before or object_names_after != object_names_before:
    raise RuntimeError("A validacao detectou objeto apagado, criado ou renomeado.")
if transforms_changed:
    raise RuntimeError("Transformacoes alteradas: " + ", ".join(transforms_changed))
if modifiers_changed:
    raise RuntimeError("Modificadores alterados: " + ", ".join(modifiers_changed))
if constraints_changed:
    raise RuntimeError("Restricoes alteradas: " + ", ".join(constraints_changed))
if protected_links_changed:
    raise RuntimeError(
        "Parenting, dados, materiais, instancias ou visibilidade alterados: "
        + ", ".join(protected_links_changed)
    )
if collection_membership_errors:
    raise RuntimeError("Vinculos finais invalidos: " + " | ".join(collection_membership_errors))

OUTPUT.parent.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.save_as_mainfile(filepath=str(OUTPUT), check_existing=False)

source_hash_after = sha256(SOURCE)
if source_hash_after != source_hash_before:
    raise RuntimeError("O hash do arquivo original mudou durante a execucao.")

command_used = f'"{bpy.app.binary_path}" --background "{SOURCE}" --python "{SCRIPT}"'

lines = [
    "RELATORIO DE ORGANIZACAO - AERIVIA BLOCKOUT",
    "=" * 48,
    f"Arquivo original: {SOURCE}",
    f"Arquivo gerado: {OUTPUT}",
    f"Script: {SCRIPT}",
    f"Blender: {bpy.app.version_string}",
    f"SHA-256 original antes: {source_hash_before}",
    f"SHA-256 original depois: {source_hash_after}",
    "Original intacto: SIM",
    f"Quantidade total de objetos: {object_count_after}",
    f"Objetos classificados/movidos: {len(classified)}",
    f"Objetos nao classificados: {len(unclassified)}",
    "",
    "QUANTIDADE DE OBJETOS POR COLECAO CRIADA",
    "-" * 48,
]
for path_key, collection in created.items():
    lines.append(f"{path_key} [datablock: {collection.name}]: {len(collection.objects)}")

lines.extend(["", "OBJETOS NAO CLASSIFICADOS", "-" * 48])
lines.extend(sorted(unclassified, key=str.casefold) or ["Nenhum."])

lines.extend(["", "OBJETOS ORIGINALMENTE EM MAIS DE UMA COLECAO", "-" * 48])
if multi_collection_before:
    for name in sorted(multi_collection_before, key=str.casefold):
        lines.append(f"{name}: {', '.join(multi_collection_before[name])}")
else:
    lines.append("Nenhum.")

lines.extend(
    [
        "",
        "VALIDACOES",
        "-" * 48,
        f"Contagem antes/depois: {object_count_before}/{object_count_after} - OK",
        "Nenhum objeto apagado, criado ou renomeado: OK",
        "Transformacoes, escala e dimensoes preservadas: OK",
        "Modificadores preservados: OK",
        "Restricoes preservadas: OK",
        "Parenting, dados, materiais e collection instances preservados: OK",
        "Visibilidade preservada: OK",
        "Um unico vinculo organizacional final por objeto classificado: OK",
        "Arquivo original preservado por comparacao SHA-256: OK",
        "",
        "COMANDO UTILIZADO",
        "-" * 48,
        command_used,
        "",
        "ERROS OU AVISOS",
        "-" * 48,
    ]
)
lines.extend(warnings or ["Nenhum."])

REPORT.write_text("\n".join(lines) + "\n", encoding="utf-8")

print("AERIVIA_ORGANIZACAO_OK")
print(f"AERIVIA_OUTPUT={OUTPUT}")
print(f"AERIVIA_REPORT={REPORT}")
print(f"AERIVIA_OBJECTS={object_count_after}")
print(f"AERIVIA_CLASSIFIED={len(classified)}")
print(f"AERIVIA_UNCLASSIFIED={len(unclassified)}")
