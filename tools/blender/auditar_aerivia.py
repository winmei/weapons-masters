"""Auditoria somente leitura da cena Aerivia para exportacao Godot."""

from __future__ import annotations

from collections import Counter, defaultdict
import hashlib
from pathlib import Path
import re
from typing import Iterable

import bpy


PROJECT_ROOT = Path(__file__).resolve().parents[2]
SOURCE = (PROJECT_ROOT / "3D" / "blender" / "spawn" / "Aerivia_Blockout_v003_organizado.blend").resolve()
REPORT = (PROJECT_ROOT / "3D" / "blender" / "spawn" / "Aerivia_auditoria_exportacao.txt").resolve()
SCRIPT = Path(__file__).resolve()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest().upper()


def format_vector(values: Iterable[float]) -> str:
    return "(" + ", ".join(f"{float(value):.6g}" for value in values) + ")"


def is_non_unit_scale(obj: bpy.types.Object, tolerance: float = 1e-6) -> bool:
    return any(abs(float(component) - 1.0) > tolerance for component in obj.scale)


loaded = Path(bpy.data.filepath).resolve()
if loaded != SOURCE:
    raise RuntimeError(f"Arquivo carregado inesperado: {loaded}; esperado: {SOURCE}")
if not SOURCE.is_file():
    raise FileNotFoundError(SOURCE)

source_hash_before = sha256(SOURCE)
objects = tuple(sorted(bpy.data.objects, key=lambda item: item.name.casefold()))


# Mapeia cada datablock de colecao para seu caminho real a partir da Scene Collection.
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
        known_paths = collection_paths.get(collection.as_pointer())
        if known_paths:
            paths.extend(known_paths)
        else:
            paths.append(collection.name)
    return tuple(sorted(set(paths)))


def is_hidden(obj: bpy.types.Object) -> bool:
    try:
        hidden_in_view_layer = bool(obj.hide_get())
    except RuntimeError:
        hidden_in_view_layer = False
    return bool(obj.hide_viewport or obj.hide_render or hidden_in_view_layer)


non_unit_scale = [obj for obj in objects if is_non_unit_scale(obj)]

exact_counts = Counter(obj.name for obj in objects)
exact_duplicate_names = sorted(name for name, count in exact_counts.items() if count > 1)
base_name_groups: dict[str, list[str]] = defaultdict(list)
for obj in objects:
    base_name = re.sub(r"\.\d{3}$", "", obj.name)
    base_name_groups[base_name].append(obj.name)
probable_duplicate_groups = {
    base: sorted(names)
    for base, names in base_name_groups.items()
    if len(names) > 1
}

multi_collection = [obj for obj in objects if len(obj.users_collection) > 1]
with_modifiers = [obj for obj in objects if obj.modifiers]
with_booleans = [obj for obj in objects if any(mod.type == "BOOLEAN" for mod in obj.modifiers)]
with_constraints = [obj for obj in objects if obj.constraints]
mesh_objects = [obj for obj in objects if obj.type == "MESH"]
without_material = [
    obj
    for obj in mesh_objects
    if not any(slot.material is not None for slot in obj.material_slots)
]

prefix_ref = [obj for obj in objects if obj.name.casefold().startswith("ref_")]
prefix_blk = [obj for obj in objects if obj.name.casefold().startswith("blk_")]
prefix_mod = [obj for obj in objects if obj.name.casefold().startswith("mod_")]
cameras = [obj for obj in objects if obj.type == "CAMERA"]
lights = [obj for obj in objects if obj.type == "LIGHT"]
empties = [obj for obj in objects if obj.type == "EMPTY"]
hidden = [obj for obj in objects if is_hidden(obj)]

mesh_statistics: list[tuple[str, int, int, int, str]] = []
total_vertices = 0
total_faces = 0
total_triangles = 0
for obj in mesh_objects:
    mesh = obj.data
    mesh.calc_loop_triangles()
    vertices = len(mesh.vertices)
    faces = len(mesh.polygons)
    triangles = len(mesh.loop_triangles)
    total_vertices += vertices
    total_faces += faces
    total_triangles += triangles
    mesh_statistics.append((obj.name, vertices, faces, triangles, mesh.name))

unique_meshes = {obj.data.as_pointer(): obj.data for obj in mesh_objects}
unique_vertices = 0
unique_faces = 0
unique_triangles = 0
for mesh in unique_meshes.values():
    mesh.calc_loop_triangles()
    unique_vertices += len(mesh.vertices)
    unique_faces += len(mesh.polygons)
    unique_triangles += len(mesh.loop_triangles)


def probable_exclusion_reasons(obj: bpy.types.Object) -> list[str]:
    reasons: list[str] = []
    paths = paths_for_object(obj)
    folded_paths = tuple(path.casefold() for path in paths)
    folded_name = obj.name.casefold()

    if obj.type == "CAMERA":
        reasons.append("camera")
    elif obj.type == "LIGHT":
        reasons.append("luz")
    elif obj.type == "EMPTY":
        reasons.append("vazio/guia")
    elif obj.type != "MESH":
        reasons.append(f"tipo nao exportavel para o mapa: {obj.type}")
    if folded_name.startswith("ref_"):
        reasons.append("prefixo REF_")
    if folded_name.startswith("mod_"):
        reasons.append("kit modular MOD_ fora do mapa")
    if is_hidden(obj):
        reasons.append("objeto oculto")
    if any(path.startswith("00_referencias") for path in folded_paths):
        reasons.append("colecao de referencias")
    if any(path.startswith("09_opcionais") for path in folded_paths):
        reasons.append("colecao opcional")
    if any("kit_estrutural" in path or "kit_urbano" in path or "kit_modular" in path for path in folded_paths):
        reasons.append("colecao de kit modular")
    return list(dict.fromkeys(reasons))


excluded: dict[str, list[str]] = {}
export_candidates: list[bpy.types.Object] = []
for obj in objects:
    reasons = probable_exclusion_reasons(obj)
    if reasons:
        excluded[obj.name] = reasons
    elif obj.type == "MESH":
        export_candidates.append(obj)

source_hash_after = sha256(SOURCE)
if source_hash_after != source_hash_before:
    raise RuntimeError("O arquivo Blender mudou durante a auditoria.")

command = f'"{bpy.app.binary_path}" --background "{SOURCE}" --python "{SCRIPT}"'
lines = [
    "AUDITORIA DE EXPORTACAO - AERIVIA",
    "=" * 72,
    f"Arquivo auditado: {SOURCE}",
    f"Blender: {bpy.app.version_string}",
    f"SHA-256 antes: {source_hash_before}",
    f"SHA-256 depois: {source_hash_after}",
    "Arquivo Blender modificado: NAO",
    f"Total de objetos: {len(objects)}",
    f"Objetos de malha: {len(mesh_objects)}",
    f"Mesh datablocks unicos: {len(unique_meshes)}",
    "",
    "RESUMO",
    "-" * 72,
    f"Escala diferente de (1, 1, 1): {len(non_unit_scale)}",
    f"Nomes exatamente duplicados: {len(exact_duplicate_names)}",
    f"Grupos provaveis por sufixo .001/.002: {len(probable_duplicate_groups)}",
    f"Objetos em mais de uma colecao: {len(multi_collection)}",
    f"Objetos com modificadores: {len(with_modifiers)}",
    f"Objetos com Boolean: {len(with_booleans)}",
    f"Objetos com restricoes: {len(with_constraints)}",
    f"Objetos de malha sem material: {len(without_material)}",
    f"Objetos REF_: {len(prefix_ref)}",
    f"Objetos BLK_: {len(prefix_blk)}",
    f"Objetos MOD_: {len(prefix_mod)}",
    f"Cameras: {len(cameras)}",
    f"Luzes: {len(lights)}",
    f"Vazios: {len(empties)}",
    f"Objetos ocultos: {len(hidden)}",
    f"Provavelmente nao exportar: {len(excluded)}",
    f"Candidatos de exportacao: {len(export_candidates)}",
    "",
    "GEOMETRIA (CONTAGEM POR INSTANCIA DE OBJETO)",
    "-" * 72,
    f"Vertices: {total_vertices}",
    f"Faces: {total_faces}",
    f"Triangulos: {total_triangles}",
    "GEOMETRIA (DATABLOCKS DE MALHA UNICOS)",
    f"Vertices: {unique_vertices}",
    f"Faces: {unique_faces}",
    f"Triangulos: {unique_triangles}",
    "",
    "OBJETOS COM ESCALA DIFERENTE DE 1",
    "-" * 72,
]
lines.extend(
    (f"{obj.name}: escala={format_vector(obj.scale)}; colecoes={', '.join(paths_for_object(obj))}" for obj in non_unit_scale),
)
if not non_unit_scale:
    lines.append("Nenhum.")

lines.extend(["", "NOMES DUPLICADOS", "-" * 72])
lines.append("Duplicatas exatas: " + (", ".join(exact_duplicate_names) if exact_duplicate_names else "Nenhuma."))
if probable_duplicate_groups:
    lines.append("Grupos por nome-base ignorando sufixos numericos:")
    for base, names in sorted(probable_duplicate_groups.items()):
        lines.append(f"- {base}: {', '.join(names)}")
else:
    lines.append("Grupos por sufixo numerico: Nenhum.")

lines.extend(["", "OBJETOS EM MAIS DE UMA COLECAO", "-" * 72])
lines.extend(
    (f"{obj.name}: {', '.join(paths_for_object(obj))}" for obj in multi_collection),
)
if not multi_collection:
    lines.append("Nenhum.")

lines.extend(["", "MODIFICADORES", "-" * 72])
for obj in with_modifiers:
    detail = ", ".join(
        f"{mod.name} ({mod.type}, viewport={mod.show_viewport}, render={mod.show_render})"
        for mod in obj.modifiers
    )
    lines.append(f"{obj.name}: {detail}")
if not with_modifiers:
    lines.append("Nenhum.")

lines.extend(["", "BOOLEANS", "-" * 72])
for obj in with_booleans:
    detail = ", ".join(mod.name for mod in obj.modifiers if mod.type == "BOOLEAN")
    lines.append(f"{obj.name}: {detail}")
if not with_booleans:
    lines.append("Nenhum.")

lines.extend(["", "RESTRICOES", "-" * 72])
for obj in with_constraints:
    detail = ", ".join(f"{constraint.name} ({constraint.type})" for constraint in obj.constraints)
    lines.append(f"{obj.name}: {detail}")
if not with_constraints:
    lines.append("Nenhuma.")

lines.extend(["", "OBJETOS DE MALHA SEM MATERIAL", "-" * 72])
lines.extend(obj.name for obj in without_material)
if not without_material:
    lines.append("Nenhum.")

for title, group in (
    ("OBJETOS REF_", prefix_ref),
    ("OBJETOS BLK_", prefix_blk),
    ("OBJETOS MOD_", prefix_mod),
    ("CAMERAS", cameras),
    ("LUZES", lights),
    ("VAZIOS", empties),
    ("OBJETOS OCULTOS", hidden),
):
    lines.extend(["", title, "-" * 72])
    lines.extend(f"{obj.name}: {', '.join(paths_for_object(obj))}" for obj in group)
    if not group:
        lines.append("Nenhum.")

lines.extend(["", "PROVAVELMENTE NAO EXPORTAR", "-" * 72])
for name, reasons in excluded.items():
    lines.append(f"{name}: {', '.join(reasons)}")
if not excluded:
    lines.append("Nenhum.")

lines.extend(["", "CANDIDATOS DE EXPORTACAO", "-" * 72])
for obj in export_candidates:
    lines.append(f"{obj.name}: {', '.join(paths_for_object(obj))}")

lines.extend(["", "ESTATISTICAS POR OBJETO DE MALHA", "-" * 72])
for name, vertices, faces, triangles, mesh_name in mesh_statistics:
    lines.append(
        f"{name}: vertices={vertices}; faces={faces}; triangulos={triangles}; mesh={mesh_name}"
    )

lines.extend(["", "COMANDO", "-" * 72, command, ""])
REPORT.write_text("\n".join(lines), encoding="utf-8")

print("AERIVIA_AUDITORIA_OK")
print(f"AERIVIA_AUDITORIA_REPORT={REPORT}")
print(f"AERIVIA_OBJECTS={len(objects)}")
print(f"AERIVIA_MESHES={len(mesh_objects)}")
print(f"AERIVIA_EXPORT_CANDIDATES={len(export_candidates)}")
print(f"AERIVIA_EXCLUDED={len(excluded)}")
print(f"AERIVIA_VERTICES={total_vertices}")
print(f"AERIVIA_FACES={total_faces}")
print(f"AERIVIA_TRIANGLES={total_triangles}")
