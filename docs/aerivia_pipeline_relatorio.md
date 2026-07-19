# Pipeline Blender → Godot — Aerívia

Data: 19/07/2026
Blender: 5.1.2
Projeto Godot detectado: `client/` (Godot 4.7, C#/.NET)

## Estado final

- Auditoria Blender: concluída, somente leitura.
- Exportação GLB: concluída e validada por reimportação limpa no Blender.
- Cena de teste Godot: criada, isolada de rede e multiplayer.
- Compilação C#: concluída com 0 erros e 0 avisos.
- Importação Godot: concluída com Godot 4.7 Mono.
- Execução headless de `AeriviaTest.tscn`: concluída com todas as verificações aprovadas.
- Cena principal do projeto: `res://scenes/LoginMMORPG.tscn`.
- Destino após autenticação bem-sucedida: `res://scenes/AeriviaTest.tscn`.

O caminho pedido inicialmente, `game-client/assets/maps/aerivia/aerivia_blockout.glb`, foi adaptado para
`client/assets/maps/aerivia/aerivia_blockout.glb`, pois `game-client/` não existe e o projeto Godot real está em
`client/project.godot`.

## Segurança e backups

Backup criado antes da implementação da cena:

`backups/aerivia_pipeline_20260719_193048/`

O backup contém:

- `client/project.godot`
- `client/Weapons Masters Client.csproj`
- `client/scenes/Main.tscn`
- `client/scripts/Game/MapSetup.cs`
- `client/scripts/Game/PlayerController.cs`
- `SHA256SUMS.txt`

Os arquivos originais foram preservados nos backups antes de cada mudança. Na integração final do fluxo de login, foi
criado também `backups/aerivia_login_flow_20260719_201943/`. Servidor, protocolo, autenticação, sessão e scripts de rede
não foram removidos ou alterados.

Durante a conferência final foi detectado que o arquivo Blender havia sido salvo externamente às 19:31:04, mudando o
SHA-256 de `85B5…` para `3046…`. Nenhum comando deste pipeline salvou o `.blend`. A nova versão ficou estável e toda a
auditoria e exportação foram repetidas para evitar um GLB desatualizado.

SHA-256 final do Blender:

`3046D797CDBE2B509575E9F4E157C64F4744E47D8FE999D60B5E0A2FF93B59CA`

SHA-256 final do GLB:

`BEDB9501A1D4075CC840D6D230416979E953828EC1220C6BA5490004F83C8B40`

## Arquivos criados

### Blender e exportação

- `tools/blender/auditar_aerivia.py`
- `tools/blender/exportar_aerivia_godot.py`
- `3D/blender/spawn/Aerivia_auditoria_exportacao.txt`
- `client/assets/maps/aerivia/aerivia_blockout.glb`

### Cena Godot

- `client/scenes/AeriviaTest.tscn`
- `client/scripts/Game/AeriviaTestScene.cs`
- `client/scripts/Game/AeriviaTestPlayer.cs`
- `tools/godot/validar_aerivia.ps1`
- `client/assets/maps/aerivia/aerivia_blockout.glb.import`

### Documentação e backup

- `docs/aerivia_pipeline_relatorio.md`
- `docs/aerivia_godot_import.log`
- `docs/aerivia_godot_run.log`
- `docs/aerivia_godot_main_scene.log`
- `docs/aerivia_login_flow_startup.log`
- `docs/aerivia_login_flow_map.log`
- `backups/aerivia_pipeline_20260719_193048/`
- `backups/aerivia_main_scene_20260719_201051/`
- `backups/aerivia_login_flow_20260719_201943/`

## Arquivos modificados

Arquivos preexistentes modificados:

- `client/project.godot`: restaura o login como entrada do aplicativo.
- `client/scripts/UI/LoginScreen.cs`: envia o login aprovado para Aerívia.

```ini
run/main_scene="res://scenes/LoginMMORPG.tscn"
```

Arquivos retirados do cliente ativo, com cópias recuperáveis no backup:

- `client/scenes/Main.tscn`
- `client/scripts/Game/MapSetup.cs`
- `client/scripts/Game/MapSetup.cs.uid`

Esses arquivos compunham a arena antiga. Scripts de servidor, rede, multiplayer, sessão e o login funcional permanecem.
O GLB e o relatório de auditoria foram regenerados uma vez após a mudança externa detectada no arquivo Blender.

## Resultado da auditoria

Relatório completo: `3D/blender/spawn/Aerivia_auditoria_exportacao.txt`

| Verificação | Resultado |
|---|---:|
| Objetos totais | 243 |
| Objetos de malha | 237 |
| Vértices | 7.192 |
| Faces | 6.694 |
| Triângulos | 13.444 |
| Escala diferente de 1 | 2 |
| Nomes duplicados | 0 |
| Objetos em mais de uma coleção | 0 |
| Objetos com modificadores | 1 |
| Booleans | 1 |
| Objetos com restrições | 1 |
| Malhas sem material atribuído | 237 |
| Objetos `REF_` | 21 |
| Objetos `BLK_` | 112 |
| Objetos `MOD_` | 110 |
| Câmeras | 1 |
| Luzes | 0 |
| Vazios | 5 |
| Objetos ocultos | 7 |
| Candidatos exportados | 105 |
| Objetos excluídos | 138 |

Objetos com escala não-unitária, preservados sem aplicar transformações:

- `BLK_Limite_Sul_Rocha_04`: `(0.833333, 0.875, 0.833333)`
- `BLK_Terreno_Ruinas_Sul`: `(55, 24, 2)`

Modificador visível:

- `BLK_Santuario_Corpo`: Boolean `Booliana`.

Restrição:

- `REF_Camera_Jogador`: `TRACK_TO`; não foi exportada porque a câmera de referência foi excluída.

## Configuração de exportação

- Formato: GLB/glTF 2.0 binário.
- Escala da cena confirmada: métrica, `1 unidade = 1 metro`.
- Eixo: conversão glTF `+Y up` para compatibilidade com Godot 4.
- Seleção: somente 105 malhas aprovadas pela auditoria.
- Modificadores: avaliados no GLB (`export_apply=True`); isso preserva o Boolean visível sem aplicar transformações no `.blend`.
- Transformações: não aplicadas nem alteradas.
- Materiais/UVs/normais: exportação habilitada.
- Animações, skins e morph targets: desabilitados.
- Câmeras e luzes: desabilitadas.
- Excluídos: `REF_`, `MOD_`, câmeras, luzes, vazios, ocultos, `00_REFERENCIAS`, `09_OPCIONAIS` e coleções de kits modulares.

Validação estrutural do GLB por reimportação numa cena Blender vazia:

- 105 objetos;
- 105 malhas;
- 0 câmeras;
- 0 luzes;
- 0 vazios;
- 0 objetos `REF_`;
- 0 objetos `MOD_`;
- 0 objetos ocultos.

## Cena de teste Godot

`AeriviaTest.tscn` contém:

- instância do GLB;
- personagem provisório com cápsula de 1,8 m;
- movimento WASD e pulo com Espaço;
- câmera em terceira pessoa com `SpringArm3D` e controle pelo mouse;
- spawn em `(0, 10.55, -8.5)`, sobre `BLK_Spawn_Plataforma`, próximo ao santuário;
- colisões trimesh provisórias apenas nas superfícies caminháveis;
- piso de segurança abaixo do mapa;
- duas luzes direcionais provisórias;
- ambiente e materiais de visualização simples;
- diagnóstico headless de malhas, materiais, escala do personagem, câmera e colisões.

A cena não instancia nem referencia `NetworkManager`, `InputSender`, `PacketHandler`, `ClientPrediction`, servidor ou
qualquer outro componente multiplayer.

## Comandos executados

Auditoria:

```powershell
& "C:\Program Files\Blender Foundation\Blender 5.1\blender.exe" --background "C:\Users\bruno\Desktop\weapons-masters\weapons-masters\3D\blender\spawn\Aerivia_Blockout_v003_organizado.blend" --python "C:\Users\bruno\Desktop\weapons-masters\weapons-masters\tools\blender\auditar_aerivia.py"
```

Exportação:

```powershell
& "C:\Program Files\Blender Foundation\Blender 5.1\blender.exe" --background "C:\Users\bruno\Desktop\weapons-masters\weapons-masters\3D\blender\spawn\Aerivia_Blockout_v003_organizado.blend" --python "C:\Users\bruno\Desktop\weapons-masters\weapons-masters\tools\blender\exportar_aerivia_godot.py"
```

Compilação do cliente:

```powershell
dotnet build ".\client\Weapons Masters Client.csproj" --no-restore --nologo
```

Resultado: compilação concluída com 0 erros e 0 avisos.

Importação e validação Godot:

```powershell
& ".\tools\godot\validar_aerivia.ps1" -GodotExecutable "C:\ProgramasDev\Godot_v4.7-stable_mono_win64\Godot_v4.7-stable_mono_win64_console.exe"
```

Resultado:

```text
GODOT_VERSION=4.7.stable.mono.official.5b4e0cb0f
AERIVIA_TEST_MAP_MESHES=105
AERIVIA_TEST_GENERATED_COLLISIONS=22
AERIVIA_TEST_PLAYER_POSITION=(0, 10.55, -8.5)
AERIVIA_TEST_PLAYER_SCALE_OK=True
AERIVIA_TEST_PLAYER_HEIGHT_OK=True
AERIVIA_TEST_CAMERA_OK=True
AERIVIA_TEST_COLLISION_OK=True
AERIVIA_TEST_MATERIAL_OVERRIDES=105
AERIVIA_TEST_MATERIALS_OK=True
AERIVIA_TEST_READY
AERIVIA_GODOT_VALIDATION_OK
```

Validação do fluxo final:

- `LoginMMORPG.tscn` carregou como cena principal com código de saída 0.
- `LoginScreen.cs` compila apontando o sucesso para `AeriviaTest.tscn`.
- `AeriviaTest.tscn` carregou separadamente com código de saída 0 e todas as verificações acima aprovadas.
- Não existem mais referências ativas a `Main.tscn` ou `MapSetup` dentro de `client/`.

O validador segue as opções oficiais `--headless`, `--path`, `--import`, `--scene` e o separador `--` documentadas em
[Godot Engine — Command line tutorial](https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html).

## Erros encontrados

- Nenhum erro na auditoria Blender.
- Nenhum erro na exportação ou reimportação do GLB.
- Nenhum erro ou aviso na compilação C#.
- Nenhum erro de importação, script ou inicialização na execução Godot headless.
- A Godot 4.7 Mono apresentou um crash nativo ao tentar abrir um segundo editor `--import` enquanto o editor gráfico já
  estava aberto. O GLB já estava atualizado; o validador passou a evitar reimportações desnecessárias e usar
  `gl_compatibility`, e as duas cenas passaram nos testes de runtime.

## Limitações atuais

- A cena foi validada em modo headless; ainda é recomendada uma passagem visual manual para avaliar enquadramento e conforto da câmera.
- A autenticação real depende do servidor e de credenciais do usuário; por segurança, ela não foi automatizada. A cena
  inicial e o destino pós-login foram validados separadamente.
- Todas as 237 malhas Blender estão sem material atribuído. A cena aplica cinco materiais provisórios por categoria em tempo de execução.
- As colisões trimesh são adequadas para blockout, mas devem ser substituídas por colisões simplificadas antes de produção.
- Os dois objetos com escala não-unitária foram preservados conforme as regras e merecem revisão manual futura.
- O GLB não possui LOD, lightmaps ou otimização de draw calls nesta etapa.

## Próximos passos

1. Reexecutar a validação quando o GLB mudar:

   ```powershell
   .\tools\godot\validar_aerivia.ps1
   ```

2. Abrir `res://scenes/AeriviaTest.tscn` e fazer um teste visual do spawn, câmera e percursos.
3. Substituir colisões trimesh por shapes simplificados.
4. Criar materiais definitivos no Blender ou na Godot.
5. Integrar os nós de gameplay multiplayer preservados à cena Aerívia quando essa etapa for iniciada.
