"""Exportacao nao destrutiva do blockout de Aerivia para Godot 4 (GLB)."""

from __future__ import annotations

from collections import defaultdict
import hashlib
from pathlib import Path
from typing import Any

import bpy


PROJECT_ROOT = Path(__file__).resolve().parents[2]
SOURCE = (PROJECT_ROOT / "3D" / "blender" / "spawn" / "Aerivia_Blockout_v003_organizado.blend").resolve()
# O projeto Godot real deste repositorio se chama "client" (nao existe "game-client").
OUTPUT = (PROJECT_ROOT / "client" / "assets" / "maps" / "aerivia" / "aerivia_blockout.glb").resolve()
SCRIPT = Path(__file__).resolve()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest().upper()


def transform_snapshot(obj: bpy.types.Object) -> tuple[Any, ...]:
    return (
        tuple(float(value) for value in obj.location),
        obj.rotation_mode,
        tuple(float(value) for value in obj.rotation_euler),
        tuple(float(value) for value in obj.rotation_quaternion),
        tuple(float(value) for value in obj.rotation_axis_angle),
        tuple(float(value) for value in obj.scale),
        tuple(float(value) for value in obj.dimensions),
        tuple(float(value) for row in obj.matrix_world for value in row),
    )


loaded = Path(bpy.data.filepath).resolve()
if loaded != SOURCE:
    raise RuntimeError(f"Arquivo carregado inesperado: {loaded}; esperado: {SOURCE}")
if not SOURCE.is_file():
    raise FileNotFoundError(SOURCE)

unit_settings = bpy.context.scene.unit_settings
if unit_settings.system != "METRIC" or abs(float(unit_settings.scale_length) - 1.0) > 1e-9:
    raise RuntimeError(
        "A cena nao esta em escala metrica 1 unidade = 1 metro. "
        f"system={unit_settings.system}; scale_length={unit_settings.scale_length}"
    )

source_hash_before = sha256(SOURCE)
objects_before = tuple(bpy.data.objects)
names_before = tuple(sorted(obj.name for obj in objects_before))
transforms_before = {obj.name: transform_snapshot(obj) for obj in objects_before}

collection_paths: dict[int, list[str]] = defaultdict(list)


def walk_collections(collection: bpy.types.Collection, path: str) -> None:
    for child in collection.children:
        child_path = f"{path}/{child.name}" if path else child.name
        collection_paths[child.as_pointer()].append(child_path)
        walk_collections(child, child_path)


walk_collections(bpy.context.scene.collection, "")


def paths_for_object(obj: bpy.types.Object) -> tuple[str, ...]:
    paths: list[str] = []
    for collection in obj.users_collection:
        paths.extend(collection_paths.get(collection.as_pointer(), [collection.name]))
    return tuple(sorted(set(paths)))


def is_hidden(obj: bpy.types.Object) -> bool:
    try:
        hidden_in_view_layer = bool(obj.hide_get())
    except RuntimeError:
        hidden_in_view_layer = False
    return bool(obj.hide_viewport or obj.hide_render or hidden_in_view_layer)


def should_export(obj: bpy.types.Object) -> bool:
    if obj.type != "MESH" or is_hidden(obj):
        return False
    folded_name = obj.name.casefold()
    folded_paths = tuple(path.casefold() for path in paths_for_object(obj))
    if folded_name.startswith(("ref_", "mod_")):
        return False
    if any(path.startswith(("00_referencias", "09_opcionais")) for path in folded_paths):
        return False
    if any(
        "kit_estrutural" in path or "kit_urbano" in path or "kit_modular" in path
        for path in folded_paths
    ):
        return False
    return True


export_objects = tuple(obj for obj in objects_before if should_export(obj))
if not export_objects:
    raise RuntimeError("Nenhum objeto elegivel para exportacao.")

# A selecao e alterada apenas na sessao background e nunca e salva no .blend.
bpy.ops.object.select_all(action="DESELECT")
for obj in export_objects:
    obj.select_set(True)
bpy.context.view_layer.objects.active = export_objects[0]

OUTPUT.parent.mkdir(parents=True, exist_ok=True)
export_properties = {prop.identifier for prop in bpy.ops.export_scene.gltf.get_rna_type().properties}
requested_options: dict[str, Any] = {
    "filepath": str(OUTPUT),
    "export_format": "GLB",
    "use_selection": True,
    "use_visible": False,
    "use_renderable": False,
    "export_cameras": False,
    "export_lights": False,
    "export_yup": True,
    # Este parametro avalia modificadores visiveis no GLB; nao aplica transforms
    # nem altera o datablock original. E necessario para o Boolean do santuario.
    "export_apply": True,
    "export_materials": "EXPORT",
    "export_image_format": "AUTO",
    "export_texcoords": True,
    "export_normals": True,
    "export_tangents": False,
    "export_attributes": False,
    "export_extras": False,
    "export_animations": False,
    "export_skins": False,
    "export_morph": False,
}
export_options = {
    key: value for key, value in requested_options.items() if key in export_properties
}
missing_required = {
    "filepath",
    "export_format",
    "use_selection",
    "export_cameras",
    "export_lights",
    "export_yup",
    "export_apply",
} - export_options.keys()
if missing_required:
    raise RuntimeError(
        "Blender sem opcoes glTF obrigatorias: " + ", ".join(sorted(missing_required))
    )

result = bpy.ops.export_scene.gltf(**export_options)
if "FINISHED" not in result:
    raise RuntimeError(f"Exportacao glTF nao finalizada: {result}")
if not OUTPUT.is_file() or OUTPUT.stat().st_size <= 20:
    raise RuntimeError(f"GLB ausente ou invalido: {OUTPUT}")

names_after = tuple(sorted(obj.name for obj in bpy.data.objects))
transforms_changed = sorted(
    obj.name
    for obj in bpy.data.objects
    if transforms_before[obj.name] != transform_snapshot(obj)
)
if names_after != names_before:
    raise RuntimeError("Objetos foram criados, apagados ou renomeados durante a exportacao.")
if transforms_changed:
    raise RuntimeError(
        "Transformacoes mudaram durante a exportacao: " + ", ".join(transforms_changed)
    )

source_hash_after = sha256(SOURCE)
if source_hash_after != source_hash_before:
    raise RuntimeError("O arquivo Blender foi modificado durante a exportacao.")

print("AERIVIA_EXPORT_OK")
print(f"AERIVIA_EXPORT_OUTPUT={OUTPUT}")
print(f"AERIVIA_EXPORT_OBJECTS={len(export_objects)}")
print(f"AERIVIA_EXPORT_BYTES={OUTPUT.stat().st_size}")
print(f"AERIVIA_SOURCE_HASH={source_hash_after}")
print("AERIVIA_EXPORT_OPTIONS=" + repr(export_options))
print("AERIVIA_EXPORT_NAMES=" + "|".join(obj.name for obj in export_objects))
