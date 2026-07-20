"""Cria uma nova versao do blockout do personagem principal de Aerivia.

Execute em background com --factory-startup e confirme usando
``-- --aerivia-generate``. O script nunca abre ou altera os mapas nem
sobrescreve uma versao existente do personagem.
"""

from __future__ import annotations

import math
import re
import sys
from pathlib import Path
from typing import Iterable

import bpy
from mathutils import Vector


PROJECT_ROOT = Path(__file__).resolve().parents[2]
OUTPUT_DIR = PROJECT_ROOT / "3D" / "blender" / "characters"
SCRIPT_PATH = Path(__file__).resolve()
ASSET_BASENAME = "Aerivia_MainCharacter"
CONFIRMATION_FLAG = "--aerivia-generate"

CHARACTER_COLLECTION = "01_CHARACTER"
RIG_COLLECTION = "02_RIG"
EQUIPMENT_COLLECTION = "03_EQUIPMENT"
PREVIEW_COLLECTION = "90_PREVIEW"


SCRIPT_ARGS = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
if not bpy.app.background or CONFIRMATION_FLAG not in SCRIPT_ARGS or bpy.data.filepath:
    raise RuntimeError(
        "Seguranca: execute em background, com --factory-startup e passe "
        f"'-- {CONFIRMATION_FLAG}'. Nenhum .blend pode estar carregado."
    )

OUTPUT_DIR.mkdir(parents=True, exist_ok=True)


def version_paths(version: str) -> tuple[Path, Path, Path]:
    return (
        OUTPUT_DIR / f"{ASSET_BASENAME}_{version}.blend",
        OUTPUT_DIR / f"{ASSET_BASENAME}_{version}_preview.png",
        OUTPUT_DIR / f"{ASSET_BASENAME}_{version}_relatorio.txt",
    )


requested_version = next(
    (
        argument.split("=", 1)[1]
        for argument in SCRIPT_ARGS
        if argument.startswith("--version=")
    ),
    None,
)
if requested_version and not re.fullmatch(r"v\d{3}", requested_version):
    raise RuntimeError("Versao invalida. Use o formato --version=v002.")

if requested_version:
    VERSION = requested_version
else:
    version_number = 1
    while any(path.exists() for path in version_paths(f"v{version_number:03d}")):
        version_number += 1
    VERSION = f"v{version_number:03d}"

BLEND_PATH, PREVIEW_PATH, REPORT_PATH = version_paths(VERSION)
existing_outputs = [path for path in (BLEND_PATH, PREVIEW_PATH, REPORT_PATH) if path.exists()]
if existing_outputs:
    raise FileExistsError(
        "Seguranca: a versao solicitada ja existe; nenhum arquivo foi sobrescrito: "
        + ", ".join(str(path) for path in existing_outputs)
    )


def clear_factory_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for collection in tuple(bpy.data.collections):
        bpy.data.collections.remove(collection)


def create_collection(name: str) -> bpy.types.Collection:
    collection = bpy.data.collections.new(name)
    bpy.context.scene.collection.children.link(collection)
    return collection


def move_to_collection(obj: bpy.types.Object, collection: bpy.types.Collection) -> None:
    if collection not in obj.users_collection:
        collection.objects.link(obj)
    for current in tuple(obj.users_collection):
        if current != collection:
            current.objects.unlink(obj)


def make_material(
    name: str,
    color: tuple[float, float, float, float],
    metallic: float = 0.0,
    roughness: float = 0.65,
    emission: tuple[float, float, float, float] | None = None,
    emission_strength: float = 0.0,
) -> bpy.types.Material:
    material = bpy.data.materials.new(name)
    material.diffuse_color = color
    material.use_nodes = True
    principled = material.node_tree.nodes.get("Principled BSDF")
    if principled:
        principled.inputs["Base Color"].default_value = color
        principled.inputs["Metallic"].default_value = metallic
        principled.inputs["Roughness"].default_value = roughness
        if emission is not None:
            principled.inputs["Emission Color"].default_value = emission
            principled.inputs["Emission Strength"].default_value = emission_strength
    return material


def apply_transforms(obj: bpy.types.Object) -> None:
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
    obj.select_set(False)


def assign_material(obj: bpy.types.Object, material: bpy.types.Material) -> None:
    obj.data.materials.append(material)


def weight_entire_object(obj: bpy.types.Object, bone_name: str) -> None:
    group = obj.vertex_groups.new(name=bone_name)
    group.add(range(len(obj.data.vertices)), 1.0, "REPLACE")
    obj["deform_bone"] = bone_name


def add_cube_part(
    name: str,
    location: Iterable[float],
    dimensions: Iterable[float],
    material: bpy.types.Material,
    bone_name: str,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_cube_add(location=location)
    obj = bpy.context.object
    obj.name = name
    obj.dimensions = dimensions
    apply_transforms(obj)
    assign_material(obj, material)
    weight_entire_object(obj, bone_name)
    return obj


def add_uv_sphere_part(
    name: str,
    location: Iterable[float],
    scale: Iterable[float],
    material: bpy.types.Material,
    bone_name: str,
    segments: int = 16,
    rings: int = 8,
) -> bpy.types.Object:
    bpy.ops.mesh.primitive_uv_sphere_add(
        segments=segments,
        ring_count=rings,
        location=location,
    )
    obj = bpy.context.object
    obj.name = name
    obj.scale = scale
    apply_transforms(obj)
    assign_material(obj, material)
    weight_entire_object(obj, bone_name)
    return obj


def add_cylinder_part(
    name: str,
    start: Iterable[float],
    end: Iterable[float],
    radius: float,
    material: bpy.types.Material,
    bone_name: str,
    vertices: int = 12,
) -> bpy.types.Object:
    start_vector = Vector(start)
    end_vector = Vector(end)
    direction = end_vector - start_vector
    midpoint = (start_vector + end_vector) * 0.5
    bpy.ops.mesh.primitive_cylinder_add(
        vertices=vertices,
        radius=radius,
        depth=direction.length,
        location=midpoint,
    )
    obj = bpy.context.object
    obj.name = name
    obj.rotation_mode = "QUATERNION"
    obj.rotation_quaternion = direction.to_track_quat("Z", "Y")
    apply_transforms(obj)
    obj.rotation_mode = "XYZ"
    assign_material(obj, material)
    weight_entire_object(obj, bone_name)
    return obj


def add_frustum_part(
    name: str,
    z_bottom: float,
    z_top: float,
    half_width_bottom: float,
    half_width_top: float,
    half_depth_bottom: float,
    half_depth_top: float,
    material: bpy.types.Material,
    bone_name: str,
) -> bpy.types.Object:
    vertices = [
        (-half_width_bottom, -half_depth_bottom, z_bottom),
        (half_width_bottom, -half_depth_bottom, z_bottom),
        (half_width_bottom, half_depth_bottom, z_bottom),
        (-half_width_bottom, half_depth_bottom, z_bottom),
        (-half_width_top, -half_depth_top, z_top),
        (half_width_top, -half_depth_top, z_top),
        (half_width_top, half_depth_top, z_top),
        (-half_width_top, half_depth_top, z_top),
    ]
    faces = [
        (0, 1, 2, 3),
        (4, 7, 6, 5),
        (0, 4, 5, 1),
        (1, 5, 6, 2),
        (2, 6, 7, 3),
        (4, 0, 3, 7),
    ]
    mesh = bpy.data.meshes.new(f"{name}_Mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    assign_material(obj, material)
    weight_entire_object(obj, bone_name)
    return obj


def add_front_panel_part(
    name: str,
    z_bottom: float,
    z_top: float,
    half_width_bottom: float,
    half_width_top: float,
    front_y_bottom: float,
    front_y_top: float,
    thickness: float,
    material: bpy.types.Material,
    bone_name: str,
) -> bpy.types.Object:
    """Cria um painel trapezoidal fino acompanhando a frente do torso."""
    vertices = [
        (-half_width_bottom, front_y_bottom, z_bottom),
        (half_width_bottom, front_y_bottom, z_bottom),
        (-half_width_top, front_y_top, z_top),
        (half_width_top, front_y_top, z_top),
        (-half_width_bottom, front_y_bottom + thickness, z_bottom),
        (half_width_bottom, front_y_bottom + thickness, z_bottom),
        (-half_width_top, front_y_top + thickness, z_top),
        (half_width_top, front_y_top + thickness, z_top),
    ]
    faces = [
        (0, 1, 3, 2),
        (4, 6, 7, 5),
        (0, 4, 5, 1),
        (2, 3, 7, 6),
        (0, 2, 6, 4),
        (1, 5, 7, 3),
    ]
    mesh = bpy.data.meshes.new(f"{name}_Mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    assign_material(obj, material)
    weight_entire_object(obj, bone_name)
    return obj


def consolidate_material_slots(
    obj: bpy.types.Object,
    ordered_materials: list[bpy.types.Material],
) -> None:
    face_materials = [
        obj.material_slots[polygon.material_index].material
        for polygon in obj.data.polygons
    ]
    obj.data.materials.clear()
    for material in ordered_materials:
        obj.data.materials.append(material)
    indices = {material.name: index for index, material in enumerate(ordered_materials)}
    for polygon, material in zip(obj.data.polygons, face_materials):
        polygon.material_index = indices[material.name]


def join_parts(
    parts: list[bpy.types.Object],
    name: str,
    materials: list[bpy.types.Material],
) -> bpy.types.Object:
    bpy.ops.object.select_all(action="DESELECT")
    for part in parts:
        part.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    joined = bpy.context.object
    joined.name = name
    joined.data.name = f"{name}_Mesh"
    bpy.context.scene.cursor.location = (0.0, 0.0, 0.0)
    bpy.ops.object.origin_set(type="ORIGIN_CURSOR")
    consolidate_material_slots(joined, materials)
    joined.select_set(False)
    return joined


def create_bone(
    armature_data: bpy.types.Armature,
    name: str,
    head: Iterable[float],
    tail: Iterable[float],
    parent: bpy.types.EditBone | None = None,
    connected: bool = False,
) -> bpy.types.EditBone:
    bone = armature_data.edit_bones.new(name)
    bone.head = head
    bone.tail = tail
    bone.parent = parent
    bone.use_connect = connected
    bone.use_deform = name != "root"
    return bone


def create_armature(collection: bpy.types.Collection) -> bpy.types.Object:
    data = bpy.data.armatures.new("RIG_Aerivia_Main_Armature")
    armature = bpy.data.objects.new("RIG_Aerivia_Main", data)
    collection.objects.link(armature)
    armature.show_in_front = True
    armature.display_type = "WIRE"
    bpy.context.view_layer.objects.active = armature
    armature.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")

    root = create_bone(data, "root", (0, 0, 0), (0, 0, 0.12))
    pelvis = create_bone(data, "pelvis", (0, 0, 0.92), (0, 0, 1.10), root)
    spine = create_bone(data, "spine", (0, 0, 1.10), (0, 0, 1.32), pelvis, True)
    chest = create_bone(data, "chest", (0, 0, 1.32), (0, 0, 1.49), spine, True)
    neck = create_bone(data, "neck", (0, 0, 1.49), (0, 0, 1.58), chest, True)
    create_bone(data, "head", (0, 0, 1.58), (0, 0, 1.80), neck, True)

    for suffix, side in (("L", 1.0), ("R", -1.0)):
        upper_arm = create_bone(
            data,
            f"upper_arm.{suffix}",
            (0.20 * side, 0, 1.44),
            (0.47 * side, 0, 1.29),
            chest,
        )
        forearm = create_bone(
            data,
            f"forearm.{suffix}",
            upper_arm.tail,
            (0.69 * side, 0, 1.12),
            upper_arm,
            True,
        )
        create_bone(
            data,
            f"hand.{suffix}",
            forearm.tail,
            (0.78 * side, 0, 1.06),
            forearm,
            True,
        )

        thigh = create_bone(
            data,
            f"thigh.{suffix}",
            (0.11 * side, 0, 1.00),
            (0.11 * side, 0, 0.61),
            pelvis,
        )
        shin = create_bone(
            data,
            f"shin.{suffix}",
            thigh.tail,
            (0.11 * side, 0, 0.18),
            thigh,
            True,
        )
        create_bone(
            data,
            f"foot.{suffix}",
            shin.tail,
            (0.11 * side, -0.20, 0.08),
            shin,
        )

    bpy.ops.object.mode_set(mode="OBJECT")
    armature.select_set(False)
    armature["character_height_m"] = 1.85
    armature["forward_axis_blender"] = "-Y"
    armature["asset_status"] = f"STYLIZED_BLOCKOUT_{VERSION.upper()}"
    return armature


def build_character_mesh(
    armature: bpy.types.Object,
    collection: bpy.types.Collection,
    materials: dict[str, bpy.types.Material],
) -> bpy.types.Object:
    parts: list[bpy.types.Object] = []

    # Silhueta feminina estilizada: shorts, cintura marcada e top sem mangas.
    parts.append(add_frustum_part("CHR_ShortsWaist", 0.98, 1.12, 0.19, 0.155, 0.105, 0.085, materials["cloth"], "pelvis"))
    parts.append(add_frustum_part("CHR_TorsoLower", 1.10, 1.31, 0.155, 0.195, 0.083, 0.105, materials["cloth"], "spine"))
    parts.append(add_frustum_part("CHR_TorsoUpper", 1.31, 1.49, 0.195, 0.215, 0.105, 0.108, materials["cloth"], "chest"))
    parts.append(add_cube_part("CHR_Belt", (0, 0, 1.085), (0.39, 0.215, 0.052), materials["leather"], "pelvis"))
    parts.append(add_cube_part("CHR_BeltBuckle", (0, -0.119, 1.085), (0.075, 0.025, 0.060), materials["metal"], "pelvis"))

    # Painel branco central e filetes prateados inspirados na referencia.
    parts.append(add_front_panel_part("CHR_WhitePanelLower", 1.12, 1.31, 0.060, 0.078, -0.092, -0.114, 0.012, materials["white"], "spine"))
    parts.append(add_front_panel_part("CHR_WhitePanelUpper", 1.31, 1.46, 0.078, 0.105, -0.114, -0.117, 0.012, materials["white"], "chest"))
    for suffix, side in (("L", 1.0), ("R", -1.0)):
        parts.append(add_cylinder_part(f"CHR_TorsoTrimLower_{suffix}", (0.092 * side, -0.100, 1.12), (0.108 * side, -0.120, 1.31), 0.008, materials["metal"], "spine", 8))
        parts.append(add_cylinder_part(f"CHR_TorsoTrimUpper_{suffix}", (0.108 * side, -0.120, 1.31), (0.135 * side, -0.120, 1.47), 0.008, materials["metal"], "chest", 8))

    parts.append(add_cylinder_part("CHR_Neck", (0, 0, 1.48), (0, 0, 1.60), 0.052, materials["skin"], "neck"))
    parts.append(add_cylinder_part("CHR_HighCollar", (0, 0, 1.49), (0, 0, 1.565), 0.067, materials["cloth"], "neck", 16))
    parts.append(add_cylinder_part("CHR_CollarTrim", (0, 0, 1.555), (0, 0, 1.575), 0.071, materials["metal"], "neck", 16))

    # Cabeca, olhos grandes e cabelo castanho com dois rabos laterais.
    parts.append(add_uv_sphere_part("CHR_Head", (0, -0.004, 1.70), (0.135, 0.115, 0.160), materials["skin"], "head", 20, 12))
    parts.append(add_uv_sphere_part("CHR_Ear_L", (0.132, 0, 1.70), (0.025, 0.018, 0.040), materials["skin"], "head", 12, 6))
    parts.append(add_uv_sphere_part("CHR_Ear_R", (-0.132, 0, 1.70), (0.025, 0.018, 0.040), materials["skin"], "head", 12, 6))
    parts.append(add_cube_part("CHR_Nose", (0, -0.117, 1.687), (0.030, 0.025, 0.042), materials["skin"], "head"))
    parts.append(add_uv_sphere_part("CHR_HairCap", (0, 0.020, 1.790), (0.148, 0.125, 0.105), materials["hair"], "head", 20, 10))
    for suffix, side in (("L", 1.0), ("R", -1.0)):
        parts.append(add_uv_sphere_part(f"CHR_PonytailRoot_{suffix}", (0.145 * side, 0.015, 1.790), (0.052, 0.052, 0.055), materials["cloth"], "head", 12, 6))
        parts.append(add_uv_sphere_part(f"CHR_PonytailUpper_{suffix}", (0.172 * side, 0.025, 1.705), (0.068, 0.063, 0.105), materials["hair"], "head", 14, 8))
        parts.append(add_uv_sphere_part(f"CHR_PonytailLower_{suffix}", (0.188 * side, 0.035, 1.605), (0.058, 0.057, 0.095), materials["hair"], "head", 14, 8))
    bang_main = add_front_panel_part("CHR_BangMain", 1.710, 1.815, 0.025, 0.092, -0.130, -0.116, 0.008, materials["hair"], "head")
    bang_main.location.x = -0.025
    parts.append(bang_main)
    bang_side = add_front_panel_part("CHR_BangSide", 1.690, 1.800, 0.014, 0.042, -0.126, -0.112, 0.008, materials["hair"], "head")
    bang_side.location.x = 0.092
    parts.append(bang_side)
    for suffix, side in (("L", 1.0), ("R", -1.0)):
        parts.append(add_uv_sphere_part(f"CHR_EyeWhite_{suffix}", (0.047 * side, -0.111, 1.715), (0.030, 0.012, 0.022), materials["eye_white"], "head", 12, 6))
        parts.append(add_uv_sphere_part(f"CHR_EyePupil_{suffix}", (0.047 * side, -0.123, 1.714), (0.013, 0.007, 0.015), materials["eye_dark"], "head", 12, 6))
        parts.append(add_cylinder_part(f"CHR_Eyebrow_{suffix}", (0.018 * side, -0.121, 1.755), (0.073 * side, -0.116, 1.750), 0.006, materials["hair"], "head", 8))
    parts.append(add_cube_part("CHR_Mouth", (0, -0.121, 1.655), (0.048, 0.008, 0.009), materials["lip"], "head"))

    for suffix, side in (("L", 1.0), ("R", -1.0)):
        shoulder = (0.205 * side, 0, 1.44)
        elbow = (0.47 * side, 0, 1.29)
        wrist = (0.69 * side, 0, 1.12)
        hand = (0.74 * side, 0, 1.085)
        parts.append(add_cylinder_part(f"CHR_UpperArm_{suffix}", shoulder, elbow, 0.052, materials["skin"], f"upper_arm.{suffix}", 12))
        parts.append(add_cylinder_part(f"CHR_Forearm_{suffix}", elbow, wrist, 0.045, materials["skin"], f"forearm.{suffix}", 12))
        parts.append(add_cylinder_part(f"CHR_Glove_{suffix}", Vector(elbow).lerp(Vector(wrist), 0.72), Vector(elbow).lerp(Vector(wrist), 0.98), 0.052, materials["cloth"], f"forearm.{suffix}", 12))
        parts.append(add_cylinder_part(f"CHR_GloveCuff_{suffix}", Vector(elbow).lerp(Vector(wrist), 0.64), Vector(elbow).lerp(Vector(wrist), 0.73), 0.059, materials["metal"], f"forearm.{suffix}", 12))
        parts.append(add_uv_sphere_part(f"CHR_Hand_{suffix}", hand, (0.052, 0.043, 0.062), materials["skin"], f"hand.{suffix}", 12, 6))
        parts.append(add_uv_sphere_part(f"CHR_HandGuard_{suffix}", (hand[0], hand[1] + 0.006, hand[2] + 0.012), (0.055, 0.045, 0.045), materials["cloth"], f"hand.{suffix}", 12, 6))

        hip = (0.11 * side, 0, 1.00)
        knee = (0.11 * side, 0, 0.61)
        ankle = (0.11 * side, 0, 0.18)
        parts.append(add_cylinder_part(f"CHR_Thigh_{suffix}", hip, knee, 0.078, materials["skin"], f"thigh.{suffix}", 12))
        parts.append(add_cylinder_part(f"CHR_Shin_{suffix}", knee, ankle, 0.064, materials["skin"], f"shin.{suffix}", 12))
        parts.append(add_cube_part(f"CHR_ShortsLeg_{suffix}", (0.11 * side, 0, 0.965), (0.185, 0.205, 0.155), materials["cloth"], f"thigh.{suffix}"))
        parts.append(add_cube_part(f"CHR_ShortsTrim_{suffix}", (0.11 * side, 0, 0.885), (0.188, 0.208, 0.030), materials["white"], f"thigh.{suffix}"))
        parts.append(add_cylinder_part(f"CHR_Boot_{suffix}", ankle, (0.11 * side, 0, 0.43), 0.073, materials["cloth"], f"shin.{suffix}", 12))
        parts.append(add_cylinder_part(f"CHR_BootCuff_{suffix}", (0.11 * side, 0, 0.40), (0.11 * side, 0, 0.47), 0.083, materials["white"], f"shin.{suffix}", 12))
        parts.append(add_cube_part(f"CHR_BootFrontTrim_{suffix}", (0.11 * side, -0.073, 0.30), (0.026, 0.014, 0.22), materials["metal"], f"shin.{suffix}"))
        parts.append(add_cube_part(f"CHR_Foot_{suffix}", (0.11 * side, -0.070, 0.095), (0.145, 0.270, 0.130), materials["cloth"], f"foot.{suffix}"))
        parts.append(add_cube_part(f"CHR_BootToe_{suffix}", (0.11 * side, -0.165, 0.095), (0.148, 0.105, 0.132), materials["white"], f"foot.{suffix}"))
        parts.append(add_cube_part(f"CHR_BootHeel_{suffix}", (0.11 * side, 0.055, 0.055), (0.105, 0.080, 0.105), materials["leather"], f"foot.{suffix}"))

    # Broche prateado com nucleo azul no colar.
    bpy.ops.mesh.primitive_cone_add(vertices=6, radius1=0.052, radius2=0.018, depth=0.060, location=(0, -0.135, 1.495), rotation=(math.radians(90), 0, 0))
    crystal = bpy.context.object
    crystal.name = "CHR_CollarBrooch"
    apply_transforms(crystal)
    assign_material(crystal, materials["glow"])
    weight_entire_object(crystal, "chest")
    parts.append(crystal)

    ordered_materials = [
        materials["skin"],
        materials["cloth"],
        materials["cloth_dark"],
        materials["white"],
        materials["leather"],
        materials["metal"],
        materials["hair"],
        materials["eye_white"],
        materials["eye_dark"],
        materials["lip"],
        materials["glow"],
    ]
    body = join_parts(parts, "CHR_Aerivia_Main_Body", ordered_materials)
    move_to_collection(body, collection)
    body.parent = armature
    modifier = body.modifiers.new("Armature", "ARMATURE")
    modifier.object = armature
    modifier.use_deform_preserve_volume = True
    body["asset_role"] = "MAIN_CHARACTER_VISUAL"
    body["rigging_quality"] = "RIGID_BLOCKOUT_WEIGHTS"
    body["visual_direction"] = "FEMALE_TEAL_WHITE_SILVER_TWIN_TAILS"
    return body


def build_sword(
    armature: bpy.types.Object,
    collection: bpy.types.Collection,
    materials: dict[str, bpy.types.Material],
) -> bpy.types.Object:
    parts: list[bpy.types.Object] = []
    # A espada e criada em world space ao lado da mao direita e depois bone-parented.
    parts.append(add_cube_part("EQP_Blade", (-0.745, -0.015, 0.68), (0.07, 0.025, 0.68), materials["metal"], "hand.R"))
    parts.append(add_cube_part("EQP_Guard", (-0.745, -0.015, 1.025), (0.24, 0.05, 0.045), materials["metal_dark"], "hand.R"))
    parts.append(add_cube_part("EQP_Grip", (-0.745, -0.015, 1.13), (0.045, 0.045, 0.19), materials["leather"], "hand.R"))
    parts.append(add_uv_sphere_part("EQP_Pommel", (-0.745, -0.015, 1.245), (0.055, 0.055, 0.055), materials["glow"], "hand.R", 12, 6))
    sword = join_parts(
        parts,
        "EQP_Aerivia_Sword_Primary",
        [materials["metal"], materials["metal_dark"], materials["leather"], materials["glow"]],
    )
    # A espada nao usa skin: os grupos auxiliares sao removidos e ela segue o osso.
    sword.vertex_groups.clear()
    move_to_collection(sword, collection)
    world_matrix = sword.matrix_world.copy()
    sword.parent = armature
    sword.parent_type = "BONE"
    sword.parent_bone = "hand.R"
    sword.matrix_world = world_matrix
    sword["equipment_slot"] = "RIGHT_HAND"
    return sword


def reset_pose(armature: bpy.types.Object) -> None:
    for pose_bone in armature.pose.bones:
        pose_bone.rotation_mode = "XYZ"
        pose_bone.rotation_euler = (0.0, 0.0, 0.0)
        pose_bone.location = (0.0, 0.0, 0.0)
        pose_bone.scale = (1.0, 1.0, 1.0)


def set_rotation(
    armature: bpy.types.Object,
    bone_name: str,
    rotation_degrees: tuple[float, float, float],
) -> None:
    bone = armature.pose.bones[bone_name]
    bone.rotation_mode = "XYZ"
    bone.rotation_euler = tuple(math.radians(value) for value in rotation_degrees)


def set_location(
    armature: bpy.types.Object,
    bone_name: str,
    location: tuple[float, float, float],
) -> None:
    bone = armature.pose.bones[bone_name]
    bone.location = location


def key_complete_pose(armature: bpy.types.Object, frame: int) -> None:
    """Grava todos os canais para impedir poses residuais entre Actions."""
    for bone in armature.pose.bones:
        bone.keyframe_insert("location", frame=frame, group=bone.name)
        bone.keyframe_insert("rotation_euler", frame=frame, group=bone.name)
        bone.keyframe_insert("scale", frame=frame, group=bone.name)


def new_action(armature: bpy.types.Object, name: str) -> bpy.types.Action:
    action = bpy.data.actions.new(name=name)
    action.use_fake_user = True
    armature.animation_data_create()
    armature.animation_data.action = action
    reset_pose(armature)
    return action


def create_animations(armature: bpy.types.Object) -> dict[str, bpy.types.Action]:
    actions: dict[str, bpy.types.Action] = {}

    reset = new_action(armature, "RESET")
    key_complete_pose(armature, 1)
    reset["loop"] = False
    actions["RESET"] = reset

    idle = new_action(armature, "Idle")
    for frame, bob, chest_tilt, arm_roll in ((1, 0.0, 0.0, 0.0), (18, 0.012, 1.8, 2.0), (36, 0.0, 0.0, 0.0)):
        reset_pose(armature)
        set_location(armature, "root", (0, 0, bob))
        set_rotation(armature, "chest", (chest_tilt, 0, 0))
        set_rotation(armature, "upper_arm.L", (0, arm_roll, 0))
        set_rotation(armature, "upper_arm.R", (0, -arm_roll, 0))
        key_complete_pose(armature, frame)
    idle["loop"] = True
    actions["Idle"] = idle

    walk = new_action(armature, "Walk")
    walk_keys = (
        (1, 25.0, -25.0, -18.0, 18.0, 0.0),
        (7, 0.0, 0.0, 0.0, 0.0, 0.018),
        (13, -25.0, 25.0, 18.0, -18.0, 0.0),
        (19, 0.0, 0.0, 0.0, 0.0, 0.018),
        (25, 25.0, -25.0, -18.0, 18.0, 0.0),
    )
    for frame, leg_l, leg_r, arm_l, arm_r, bob in walk_keys:
        reset_pose(armature)
        set_rotation(armature, "thigh.L", (leg_l, 0, 0))
        set_rotation(armature, "thigh.R", (leg_r, 0, 0))
        set_rotation(armature, "upper_arm.L", (arm_l, 0, 0))
        set_rotation(armature, "upper_arm.R", (arm_r, 0, 0))
        set_rotation(armature, "shin.L", (max(0.0, -leg_l * 0.65), 0, 0))
        set_rotation(armature, "shin.R", (max(0.0, -leg_r * 0.65), 0, 0))
        set_location(armature, "root", (0, 0, bob))
        key_complete_pose(armature, frame)
    walk["loop"] = True
    actions["Walk"] = walk

    run = new_action(armature, "Run")
    run_keys = (
        (1, 42.0, -42.0, -32.0, 32.0, 0.0),
        (5, 0.0, 0.0, 0.0, 0.0, 0.035),
        (10, -42.0, 42.0, 32.0, -32.0, 0.0),
        (14, 0.0, 0.0, 0.0, 0.0, 0.035),
        (19, 42.0, -42.0, -32.0, 32.0, 0.0),
    )
    for frame, leg_l, leg_r, arm_l, arm_r, bob in run_keys:
        reset_pose(armature)
        set_rotation(armature, "thigh.L", (leg_l, 0, 0))
        set_rotation(armature, "thigh.R", (leg_r, 0, 0))
        set_rotation(armature, "shin.L", (max(0.0, -leg_l * 0.8), 0, 0))
        set_rotation(armature, "shin.R", (max(0.0, -leg_r * 0.8), 0, 0))
        set_rotation(armature, "upper_arm.L", (arm_l, 0, 0))
        set_rotation(armature, "upper_arm.R", (arm_r, 0, 0))
        set_rotation(armature, "chest", (8.0, 0, 0))
        set_location(armature, "root", (0, 0, bob))
        key_complete_pose(armature, frame)
    run["loop"] = True
    actions["Run"] = run

    # Interpolacao Bezier suave para o blockout; pode ser refinada pelo animador.
    for action_name, action in actions.items():
        for slot in action.slots:
            channelbag = action.layers[0].strips[0].channelbag(slot, ensure=False)
            if channelbag:
                for fcurve in channelbag.fcurves:
                    for point in fcurve.keyframe_points:
                        point.interpolation = "BEZIER" if action_name == "Idle" else "LINEAR"

    reset_pose(armature)
    armature.animation_data.action = idle
    bpy.context.scene.frame_start = 1
    bpy.context.scene.frame_end = 36
    bpy.context.scene.frame_set(1)
    return actions


def add_preview_scene(
    collection: bpy.types.Collection,
    materials: dict[str, bpy.types.Material],
) -> None:
    bpy.ops.mesh.primitive_cylinder_add(vertices=64, radius=1.05, depth=0.05, location=(0, 0, -0.03))
    pedestal = bpy.context.object
    pedestal.name = "PREVIEW_Pedestal"
    assign_material(pedestal, materials["preview_floor"])
    move_to_collection(pedestal, collection)

    bpy.ops.mesh.primitive_torus_add(major_radius=0.78, minor_radius=0.012, major_segments=64, minor_segments=8, location=(0, 0, 0.005))
    ring = bpy.context.object
    ring.name = "PREVIEW_Aerivia_Ring"
    assign_material(ring, materials["glow"])
    move_to_collection(ring, collection)

    def add_area(name: str, location: tuple[float, float, float], energy: float, color: tuple[float, float, float], size: float) -> None:
        light_data = bpy.data.lights.new(name, type="AREA")
        light_data.energy = energy
        light_data.color = color
        light_data.shape = "DISK"
        light_data.size = size
        light = bpy.data.objects.new(name, light_data)
        collection.objects.link(light)
        light.location = location
        direction = Vector((0, 0, 1.05)) - light.location
        light.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()

    add_area("PREVIEW_Key", (-2.4, -3.0, 3.4), 900, (0.72, 0.86, 1.0), 2.2)
    add_area("PREVIEW_Fill", (2.8, -1.7, 2.0), 650, (1.0, 0.56, 0.30), 2.0)
    add_area("PREVIEW_Rim", (0.5, 2.0, 3.2), 1100, (0.18, 0.65, 1.0), 1.6)

    camera_data = bpy.data.cameras.new("PREVIEW_Camera")
    camera = bpy.data.objects.new("PREVIEW_Camera", camera_data)
    collection.objects.link(camera)
    camera.location = (2.75, -5.2, 2.35)
    camera.data.lens = 62
    target = Vector((0, 0, 0.95))
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
    bpy.context.scene.camera = camera


def configure_scene() -> None:
    scene = bpy.context.scene
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0
    scene.unit_settings.length_unit = "METERS"
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 700
    scene.render.resolution_y = 900
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(PREVIEW_PATH)
    scene.render.film_transparent = False
    scene.world.color = (0.008, 0.012, 0.025)
    scene.render.image_settings.color_mode = "RGBA"
    try:
        scene.view_settings.look = "AgX - Medium High Contrast"
    except TypeError:
        pass


def object_world_bounds(obj: bpy.types.Object) -> tuple[Vector, Vector]:
    corners = [obj.matrix_world @ Vector(corner) for corner in obj.bound_box]
    minimum = Vector(tuple(min(corner[index] for corner in corners) for index in range(3)))
    maximum = Vector(tuple(max(corner[index] for corner in corners) for index in range(3)))
    return minimum, maximum


clear_factory_scene()
configure_scene()

character_collection = create_collection(CHARACTER_COLLECTION)
rig_collection = create_collection(RIG_COLLECTION)
equipment_collection = create_collection(EQUIPMENT_COLLECTION)
preview_collection = create_collection(PREVIEW_COLLECTION)

materials = {
    "skin": make_material("MAT_Skin_Fair", (0.72, 0.46, 0.34, 1.0), roughness=0.68),
    "cloth": make_material("MAT_Aerivia_Teal", (0.025, 0.30, 0.36, 1.0), roughness=0.64),
    "cloth_dark": make_material("MAT_Teal_Shadow", (0.012, 0.11, 0.14, 1.0), roughness=0.74),
    "white": make_material("MAT_Fabric_White", (0.72, 0.78, 0.80, 1.0), roughness=0.58),
    "leather": make_material("MAT_Leather", (0.16, 0.075, 0.035, 1.0), roughness=0.82),
    "metal": make_material("MAT_Silver", (0.58, 0.68, 0.72, 1.0), metallic=0.78, roughness=0.24),
    "metal_dark": make_material("MAT_DarkMetal", (0.055, 0.07, 0.10, 1.0), metallic=0.9, roughness=0.24),
    "hair": make_material("MAT_Hair_Brown", (0.032, 0.014, 0.008, 1.0), roughness=0.56),
    "eye_white": make_material("MAT_Eye_White", (0.82, 0.84, 0.82, 1.0), roughness=0.32),
    "eye_dark": make_material("MAT_Eye_Brown", (0.022, 0.012, 0.008, 1.0), roughness=0.25),
    "lip": make_material("MAT_Lips", (0.50, 0.13, 0.12, 1.0), roughness=0.56),
    "glow": make_material(
        "MAT_Aerivia_Crystal",
        (0.015, 0.35, 0.62, 1.0),
        metallic=0.22,
        roughness=0.18,
        emission=(0.02, 0.48, 1.0, 1.0),
        emission_strength=4.0,
    ),
    "preview_floor": make_material("MAT_Preview_Floor", (0.012, 0.018, 0.032, 1.0), metallic=0.15, roughness=0.5),
}

armature = create_armature(rig_collection)
body = build_character_mesh(armature, character_collection, materials)
sword = build_sword(armature, equipment_collection, materials)
actions = create_animations(armature)
add_preview_scene(preview_collection, materials)

# Metadados para facilitar a futura automacao de exportacao.
bpy.context.scene["asset_name"] = "Aerivia Main Character"
bpy.context.scene["asset_version"] = VERSION
bpy.context.scene["target_engine"] = "Godot 4.7"
bpy.context.scene["visual_reference"] = "Female teal-white-silver twin-tail concept"
bpy.context.scene["export_forward"] = "-Y in Blender / +Z model front in glTF"
bpy.context.scene["recommended_export"] = "GLB, selected character+rig+equipment, preserve root bone"

# Validacao antes de salvar.
required_bones = {
    "root", "pelvis", "spine", "chest", "neck", "head",
    "upper_arm.L", "forearm.L", "hand.L", "upper_arm.R", "forearm.R", "hand.R",
    "thigh.L", "shin.L", "foot.L", "thigh.R", "shin.R", "foot.R",
}
actual_bones = {bone.name for bone in armature.data.bones}
missing_bones = sorted(required_bones - actual_bones)
missing_actions = sorted({"RESET", "Idle", "Walk", "Run"} - set(actions))
modifier_ok = any(modifier.type == "ARMATURE" and modifier.object == armature for modifier in body.modifiers)
weighted_groups = {group.name for group in body.vertex_groups}
missing_groups = sorted((required_bones - {"root"}) - weighted_groups)
body_minimum, body_maximum = object_world_bounds(body)
character_height = body_maximum.z - body_minimum.z

if missing_bones:
    raise RuntimeError("Ossos ausentes: " + ", ".join(missing_bones))
if missing_actions:
    raise RuntimeError("Animacoes ausentes: " + ", ".join(missing_actions))
if not modifier_ok:
    raise RuntimeError("Modificador Armature ausente ou invalido.")
if missing_groups:
    raise RuntimeError("Grupos de deformacao ausentes: " + ", ".join(missing_groups))
if not (1.70 <= character_height <= 1.90):
    raise RuntimeError(f"Altura fora do esperado: {character_height:.3f} m")

# Salva, renderiza a pre-visualizacao e salva novamente mantendo Idle ativo.
bpy.ops.wm.save_as_mainfile(filepath=str(BLEND_PATH), check_existing=False)
bpy.context.scene.render.filepath = str(PREVIEW_PATH)
bpy.ops.render.render(write_still=True)
bpy.ops.wm.save_as_mainfile(filepath=str(BLEND_PATH), check_existing=False)

triangles = sum(len(polygon.vertices) - 2 for polygon in body.data.polygons)
command = (
    f'"{bpy.app.binary_path}" --background --factory-startup '
    f'--python "{SCRIPT_PATH}" -- {CONFIRMATION_FLAG} --version={VERSION}'
)
report_lines = [
    f"RELATORIO - AERIVIA MAIN CHARACTER {VERSION.upper()}",
    "=" * 56,
    f"Arquivo Blender: {BLEND_PATH}",
    f"Preview: {PREVIEW_PATH}",
    f"Blender: {bpy.app.version_string}",
    "Direcao visual: feminina estilizada, turquesa/branco/prata, cabelo duplo.",
    f"Altura visual aproximada: {character_height:.3f} m",
    f"Vertices da malha principal: {len(body.data.vertices)}",
    f"Faces da malha principal: {len(body.data.polygons)}",
    f"Triangulos estimados: {triangles}",
    f"Materiais da malha principal: {len(body.material_slots)}",
    f"Ossos deformadores: {sum(bone.use_deform for bone in armature.data.bones)}",
    f"Total de ossos: {len(armature.data.bones)}",
    f"Grupos de vertices: {len(body.vertex_groups)}",
    f"Modificador Armature valido: {'SIM' if modifier_ok else 'NAO'}",
    f"Espada presa ao osso: {sword.parent_bone}",
    "Animacoes:",
]
for action_name, action in actions.items():
    frame_range = action.frame_range
    report_lines.append(f"- {action_name}: frames {frame_range[0]:.0f}-{frame_range[1]:.0f}")
report_lines.extend(
    [
        "",
        "COLECOES",
        f"- {CHARACTER_COLLECTION}: malha principal",
        f"- {RIG_COLLECTION}: armature",
        f"- {EQUIPMENT_COLLECTION}: espada",
        f"- {PREVIEW_COLLECTION}: camera, luzes e pedestal (nao exportar)",
        "",
        "LIMITACOES DO BLOCKOUT",
        "- Pesos rigidos por segmento; as articulacoes ainda precisam de weight paint refinado.",
        "- Rosto, maos, cabelo, roupa e armadura sao formas low-poly provisórias.",
        "- RESET, Idle, Walk e Run validam o pipeline, mas nao sao animacoes finais.",
        "- A espada e um placeholder preso a hand.R.",
        "",
        "COMANDO UTILIZADO",
        command,
        "",
    ]
)
REPORT_PATH.write_text("\n".join(report_lines), encoding="utf-8")

print("AERIVIA_CHARACTER_OK")
print(f"AERIVIA_CHARACTER_BLEND={BLEND_PATH}")
print(f"AERIVIA_CHARACTER_PREVIEW={PREVIEW_PATH}")
print(f"AERIVIA_CHARACTER_REPORT={REPORT_PATH}")
print(f"AERIVIA_CHARACTER_HEIGHT={character_height:.3f}")
print(f"AERIVIA_CHARACTER_VERTICES={len(body.data.vertices)}")
print(f"AERIVIA_CHARACTER_TRIANGLES={triangles}")
print(f"AERIVIA_CHARACTER_BONES={len(armature.data.bones)}")
print("AERIVIA_CHARACTER_ACTIONS=" + "|".join(sorted(actions)))
