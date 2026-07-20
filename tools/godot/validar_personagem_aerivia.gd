extends SceneTree

const CHARACTER_SCENE := "res://assets/characters/aerivia/aerivia_main_character.glb"
const REQUIRED_ANIMATIONS := ["Idle", "RESET", "Run", "Walk"]


func fail(message: String) -> void:
	printerr("AERIVIA_GODOT_CHARACTER_ERROR=" + message)
	quit(1)


func collect_nodes(node: Node, result: Array[Node]) -> void:
	result.append(node)
	for child in node.get_children():
		collect_nodes(child, result)


func _initialize() -> void:
	var resource := load(CHARACTER_SCENE) as PackedScene
	if resource == null:
		fail("Nao foi possivel carregar " + CHARACTER_SCENE)
		return

	var character := resource.instantiate()
	root.add_child(character)
	var nodes: Array[Node] = []
	collect_nodes(character, nodes)

	var skeletons := nodes.filter(func(node: Node) -> bool: return node is Skeleton3D)
	var meshes := nodes.filter(func(node: Node) -> bool: return node is MeshInstance3D)
	var players := nodes.filter(func(node: Node) -> bool: return node is AnimationPlayer)
	if skeletons.size() != 1:
		fail("Esperado 1 Skeleton3D, encontrado: %d" % skeletons.size())
		return
	if meshes.size() != 2:
		fail("Esperadas 2 malhas, encontradas: %d" % meshes.size())
		return
	if players.is_empty():
		fail("AnimationPlayer ausente.")
		return

	var imported_animations: Array[String] = []
	for player: AnimationPlayer in players:
		for animation_name in player.get_animation_list():
			var simple_name := String(animation_name).get_slice("/", 1)
			if simple_name.is_empty():
				simple_name = String(animation_name)
			if simple_name not in imported_animations:
				imported_animations.append(simple_name)

	for required_animation in REQUIRED_ANIMATIONS:
		if required_animation not in imported_animations:
			fail("Animacao ausente: " + required_animation)
			return

	var skeleton := skeletons[0] as Skeleton3D
	if skeleton.get_bone_count() != 18:
		fail("Esperados 18 ossos, encontrados: %d" % skeleton.get_bone_count())
		return

	print("AERIVIA_GODOT_CHARACTER_OK")
	print("AERIVIA_GODOT_CHARACTER_SCENE=" + CHARACTER_SCENE)
	print("AERIVIA_GODOT_CHARACTER_SKELETONS=%d" % skeletons.size())
	print("AERIVIA_GODOT_CHARACTER_BONES=%d" % skeleton.get_bone_count())
	print("AERIVIA_GODOT_CHARACTER_MESHES=%d" % meshes.size())
	print("AERIVIA_GODOT_CHARACTER_ANIMATIONS=" + "|".join(imported_animations))
	quit(0)
