"""Valida, sem salvar, o blockout do personagem principal de Aerivia.

Uso:
    blender Aerivia_MainCharacter_v001.blend --background --python este_arquivo.py
"""

from __future__ import annotations

import sys

import bpy


BODY_NAME = "CHR_Aerivia_Main_Body"
RIG_NAME = "RIG_Aerivia_Main"
SWORD_NAME = "EQP_Aerivia_Sword_Primary"
REQUIRED_ACTIONS = {"RESET", "Idle", "Walk", "Run"}
REQUIRED_COLLECTIONS = {"01_CHARACTER", "02_RIG", "03_EQUIPMENT", "90_PREVIEW"}
REQUIRED_BONES = {
    "root", "pelvis", "spine", "chest", "neck", "head",
    "upper_arm.L", "forearm.L", "hand.L",
    "upper_arm.R", "forearm.R", "hand.R",
    "thigh.L", "shin.L", "foot.L",
    "thigh.R", "shin.R", "foot.R",
}


def fail(message: str) -> None:
    print(f"AERIVIA_VALIDATION_ERROR={message}")
    raise RuntimeError(message)


def reset_runtime_pose(armature: bpy.types.Object) -> None:
    for bone in armature.pose.bones:
        bone.matrix_basis.identity()
    bpy.context.view_layer.update()


def action_fcurves(action: bpy.types.Action) -> list[bpy.types.FCurve]:
    curves: list[bpy.types.FCurve] = []
    for layer in action.layers:
        for strip in layer.strips:
            for slot in action.slots:
                channelbag = strip.channelbag(slot, ensure=False)
                if channelbag:
                    curves.extend(channelbag.fcurves)
    return curves


def pose_signature(armature: bpy.types.Object) -> tuple[float, ...]:
    values: list[float] = []
    for bone in armature.pose.bones:
        values.extend((*bone.location, *bone.rotation_euler, *bone.scale))
    return tuple(values)


if not bpy.data.filepath:
    fail("Nenhum arquivo .blend foi aberto.")

body = bpy.data.objects.get(BODY_NAME)
rig = bpy.data.objects.get(RIG_NAME)
sword = bpy.data.objects.get(SWORD_NAME)

if body is None or body.type != "MESH":
    fail(f"Malha principal ausente: {BODY_NAME}")
if rig is None or rig.type != "ARMATURE":
    fail(f"Armature ausente: {RIG_NAME}")
if sword is None or sword.type != "MESH":
    fail(f"Espada ausente: {SWORD_NAME}")

missing_bones = sorted(REQUIRED_BONES - {bone.name for bone in rig.data.bones})
if missing_bones:
    fail("Ossos ausentes: " + ", ".join(missing_bones))

missing_collections = sorted(
    REQUIRED_COLLECTIONS - {collection.name for collection in bpy.data.collections}
)
if missing_collections:
    fail("Colecoes ausentes: " + ", ".join(missing_collections))

missing_actions = sorted(REQUIRED_ACTIONS - {action.name for action in bpy.data.actions})
if missing_actions:
    fail("Acoes ausentes: " + ", ".join(missing_actions))

armature_modifiers = [
    modifier
    for modifier in body.modifiers
    if modifier.type == "ARMATURE" and modifier.object == rig
]
if len(armature_modifiers) != 1:
    fail("A malha deve ter exatamente um modificador Armature ligado ao rig.")

if sword.parent != rig or sword.parent_type != "BONE" or sword.parent_bone != "hand.R":
    fail("A espada nao esta corretamente vinculada ao osso hand.R.")

for obj in (body, rig, sword):
    if any(abs(value - 1.0) > 0.0001 for value in obj.scale):
        fail(f"Escala inesperada em {obj.name}: {tuple(obj.scale)}")

deform_group_indices = {
    body.vertex_groups[bone.name].index
    for bone in rig.data.bones
    if bone.use_deform and body.vertex_groups.get(bone.name)
}
unweighted_vertices: list[int] = []
non_normalized_vertices: list[int] = []
for vertex in body.data.vertices:
    deform_weights = [
        assignment.weight
        for assignment in vertex.groups
        if assignment.group in deform_group_indices
    ]
    if not deform_weights:
        unweighted_vertices.append(vertex.index)
    elif abs(sum(deform_weights) - 1.0) > 0.001:
        non_normalized_vertices.append(vertex.index)
if unweighted_vertices:
    fail(f"Vertices sem peso deformador: {len(unweighted_vertices)}")
if non_normalized_vertices:
    fail(f"Vertices com pesos nao normalizados: {len(non_normalized_vertices)}")

expected_curve_paths = {
    f'pose.bones["{bone_name}"].{property_name}'
    for bone_name in REQUIRED_BONES
    for property_name in ("location", "rotation_euler", "scale")
}
for action_name in sorted(REQUIRED_ACTIONS):
    action = bpy.data.actions[action_name]
    if not action.slots or not action.layers:
        fail(f"Acao sem dados animados: {action_name}")
    actual_curve_paths = {curve.data_path for curve in action_fcurves(action)}
    missing_curve_paths = expected_curve_paths - actual_curve_paths
    if missing_curve_paths:
        fail(
            f"Acao {action_name} sem canais completos: "
            + ", ".join(sorted(missing_curve_paths)[:5])
        )

scene = bpy.context.scene
probe_frames = {"RESET": 1, "Idle": 18, "Walk": 1, "Run": 1}
pose_magnitudes: dict[str, float] = {}
for action_name, frame in probe_frames.items():
    reset_runtime_pose(rig)
    rig.animation_data.action = bpy.data.actions[action_name]
    scene.frame_set(frame)
    magnitude = max(
        max(abs(value) for value in (*bone.location, *bone.rotation_euler))
        for bone in rig.pose.bones
    )
    if action_name == "RESET" and magnitude > 0.001:
        fail("A acao RESET nao representa a pose de repouso.")
    if action_name != "RESET" and magnitude <= 0.001:
        fail(f"A acao nao altera a pose no frame de teste: {action_name}")
    pose_magnitudes[action_name] = magnitude

for action_name in ("Idle", "Walk", "Run"):
    action = bpy.data.actions[action_name]
    start_frame, end_frame = (round(value) for value in action.frame_range)
    reset_runtime_pose(rig)
    rig.animation_data.action = action
    scene.frame_set(start_frame)
    start_signature = pose_signature(rig)
    scene.frame_set(end_frame)
    end_signature = pose_signature(rig)
    if max(abs(start - end) for start, end in zip(start_signature, end_signature)) > 0.0001:
        fail(f"A acao ciclica nao fecha corretamente: {action_name}")

reset_runtime_pose(rig)
rig.animation_data.action = bpy.data.actions["Run"]
scene.frame_set(1)
rig.animation_data.action = bpy.data.actions["Idle"]
scene.frame_set(1)
if max(abs(value) for bone in rig.pose.bones for value in (*bone.location, *bone.rotation_euler)) > 0.001:
    fail("A troca Run -> Idle deixou transformacoes residuais.")

evaluated_body = body.evaluated_get(bpy.context.evaluated_depsgraph_get())
height = evaluated_body.dimensions.z
if not 1.70 <= height <= 1.90:
    fail(f"Altura fora da faixa esperada: {height:.3f} m")

print("AERIVIA_VALIDATION_OK")
print(f"AERIVIA_VALIDATION_FILE={bpy.data.filepath}")
print(f"AERIVIA_VALIDATION_HEIGHT={height:.3f}")
print(f"AERIVIA_VALIDATION_VERTICES={len(body.data.vertices)}")
print(f"AERIVIA_VALIDATION_FACES={len(body.data.polygons)}")
print(f"AERIVIA_VALIDATION_BONES={len(rig.data.bones)}")
print(f"AERIVIA_VALIDATION_GROUPS={len(body.vertex_groups)}")
print(f"AERIVIA_VALIDATION_WEIGHTED_VERTICES={len(body.data.vertices)}")
print("AERIVIA_VALIDATION_ACTIONS=" + "|".join(sorted(REQUIRED_ACTIONS)))
print(
    "AERIVIA_VALIDATION_POSE_MAGNITUDES="
    + "|".join(f"{name}:{pose_magnitudes[name]:.4f}" for name in sorted(pose_magnitudes))
)
print("AERIVIA_VALIDATION_COLLECTIONS=" + "|".join(sorted(REQUIRED_COLLECTIONS)))
sys.stdout.flush()
