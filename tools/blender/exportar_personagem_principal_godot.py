"""Exporta somente personagem, rig, equipamento e animacoes para um GLB.

Uso:
    blender personagem.blend --background --python este_arquivo.py -- \
        --aerivia-export

Se o GLB ja existir, uma copia com data e hora e criada em backups antes da
substituicao. O arquivo Blender carregado nunca e salvo por este script.
"""

from __future__ import annotations

import shutil
import sys
from datetime import datetime
from pathlib import Path

import bpy


PROJECT_ROOT = Path(__file__).resolve().parents[2]
OUTPUT_PATH = (
    PROJECT_ROOT
    / "client"
    / "assets"
    / "characters"
    / "aerivia"
    / "aerivia_main_character.glb"
)
BACKUP_ROOT = PROJECT_ROOT / "backups" / "aerivia_character_exports"
CONFIRMATION_FLAG = "--aerivia-export"
EXPORT_OBJECTS = {
    "CHR_Aerivia_Main_Body",
    "RIG_Aerivia_Main",
    "EQP_Aerivia_Sword_Primary",
}
REQUIRED_ACTIONS = {"RESET", "Idle", "Walk", "Run"}


script_args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
if not bpy.app.background or CONFIRMATION_FLAG not in script_args:
    raise RuntimeError(
        "Seguranca: execute em background e confirme com "
        f"'-- {CONFIRMATION_FLAG}'."
    )
if not bpy.data.filepath:
    raise RuntimeError("Seguranca: nenhum arquivo Blender foi carregado.")

missing_objects = sorted(EXPORT_OBJECTS - {obj.name for obj in bpy.data.objects})
if missing_objects:
    raise RuntimeError("Objetos obrigatorios ausentes: " + ", ".join(missing_objects))

missing_actions = sorted(REQUIRED_ACTIONS - {action.name for action in bpy.data.actions})
if missing_actions:
    raise RuntimeError("Actions obrigatorias ausentes: " + ", ".join(missing_actions))

armature = bpy.data.objects["RIG_Aerivia_Main"]
if armature.type != "ARMATURE":
    raise RuntimeError("RIG_Aerivia_Main nao e uma Armature.")

OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
backup_path: Path | None = None
if OUTPUT_PATH.exists():
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_directory = BACKUP_ROOT / timestamp
    backup_directory.mkdir(parents=True, exist_ok=False)
    backup_path = backup_directory / OUTPUT_PATH.name
    shutil.copy2(OUTPUT_PATH, backup_path)

bpy.ops.object.mode_set(mode="OBJECT") if bpy.context.object and bpy.context.object.mode != "OBJECT" else None
bpy.ops.object.select_all(action="DESELECT")
for object_name in EXPORT_OBJECTS:
    bpy.data.objects[object_name].select_set(True)
bpy.context.view_layer.objects.active = armature

armature.animation_data_create()
armature.animation_data.action = bpy.data.actions["Idle"]
for bone in armature.pose.bones:
    bone.matrix_basis.identity()
bpy.context.scene.frame_set(1)

result = bpy.ops.export_scene.gltf(
    filepath=str(OUTPUT_PATH),
    check_existing=False,
    export_format="GLB",
    use_selection=True,
    export_cameras=False,
    export_lights=False,
    export_extras=True,
    export_yup=True,
    export_apply=False,
    export_materials="EXPORT",
    export_animations=True,
    export_animation_mode="ACTIONS",
    export_frame_range=True,
    export_force_sampling=True,
    export_sampling_interpolation_fallback="LINEAR",
    # O root nao deforma vertices, mas carrega o movimento vertical das Actions.
    # Como este rig ainda nao possui bones de controle, todos os 18 sao exportados.
    export_def_bones=False,
    export_leaf_bone=False,
    export_skins=True,
    export_reset_pose_bones=True,
    export_anim_single_armature=True,
    export_optimize_animation_size=True,
    export_optimize_animation_keep_anim_armature=True,
    export_morph=False,
)
if "FINISHED" not in result or not OUTPUT_PATH.exists() or OUTPUT_PATH.stat().st_size == 0:
    raise RuntimeError(f"A exportacao GLB falhou: {result}")

print("AERIVIA_CHARACTER_EXPORT_OK")
print(f"AERIVIA_CHARACTER_EXPORT_SOURCE={bpy.data.filepath}")
print(f"AERIVIA_CHARACTER_EXPORT_GLB={OUTPUT_PATH}")
print(f"AERIVIA_CHARACTER_EXPORT_BYTES={OUTPUT_PATH.stat().st_size}")
print(f"AERIVIA_CHARACTER_EXPORT_BACKUP={backup_path or 'NAO_NECESSARIO'}")
print("AERIVIA_CHARACTER_EXPORT_OBJECTS=" + "|".join(sorted(EXPORT_OBJECTS)))
print("AERIVIA_CHARACTER_EXPORT_ACTIONS=" + "|".join(sorted(REQUIRED_ACTIONS)))
sys.stdout.flush()
