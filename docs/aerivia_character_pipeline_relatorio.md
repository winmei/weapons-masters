# Pipeline do personagem principal de Aerívia

Data da validação: 19/07/2026

Blender: 5.1.2

Godot: 4.7 stable Mono

## Resultado

O blockout feminino estilizado v005 foi gerado sem sobrescrever as versões
anteriores, exportado para GLB, importado no Godot e conectado ao jogador local
da cena `AeriviaTest`.
Login, servidor e multiplayer não foram alterados.

A direção visual usa como referência `C:/Users/bruno/Desktop/AssetsFor3D/2D/Female.png`:
cabelo castanho em dois rabos, top turquesa com painel branco e acabamento
prateado, shorts, luvas e botas coordenadas.

## Arquivos principais

- `3D/blender/characters/Aerivia_MainCharacter_v005.blend`
- `3D/blender/characters/Aerivia_MainCharacter_v005_preview.png`
- `3D/blender/characters/Aerivia_MainCharacter_v005_relatorio.txt`
- `client/assets/characters/aerivia/aerivia_main_character.glb`
- `client/assets/characters/aerivia/aerivia_main_character.glb.import`
- `tools/blender/criar_personagem_principal.py`
- `tools/blender/validar_personagem_principal.py`
- `tools/blender/exportar_personagem_principal_godot.py`
- `tools/godot/validar_personagem_aerivia.gd`

## Arquivos do cliente modificados

- `client/scenes/AeriviaTest.tscn`: a cápsula visual foi substituída pelo GLB.
  A cápsula de colisão, câmera, mapa, iluminação e HUD foram preservados.
- `client/scripts/Game/AeriviaTestPlayer.cs`: seleção automática de
  `Idle`, `Walk` e `Run` conforme a velocidade, com transição de 0,15 s.

## Correções do review

- O gerador só executa em background, sem `.blend` carregado e com a
  confirmação explícita `--aerivia-generate`.
- Versões existentes não são sobrescritas; o próximo número livre é escolhido.
- Cada Action grava localização, rotação e escala de todos os 18 ossos.
- Foi adicionada a Action `RESET`.
- O validador limpa a pose antes de cada teste e verifica F-curves, fechamento
  dos ciclos, troca `Run -> Idle`, cobertura e normalização dos pesos.
- O exportador inclui somente corpo, rig e espada; exclui câmera, luzes e
  pedestal.
- O osso `root` é preservado no GLB, mesmo sem deformar vértices.
- Um GLB existente recebe backup antes de ser substituído.

## Backups

- GLB anterior:
  `backups/aerivia_character_exports/20260719_214553/aerivia_main_character.glb`
- GLB substituído pelo visual v005:
  `backups/aerivia_character_exports/20260719_215727/aerivia_main_character.glb`
- Cena e controlador anteriores:
  `backups/aerivia_character_integration/20260719_2148/`
- Cena anterior ao ajuste visual de escala:
  `backups/aerivia_character_integration/20260719_2200/`
- Os arquivos v001, v002, v003 e v004 foram preservados.

## Validação Blender

- Altura: 1,892 m.
- Vértices: 2.312.
- Faces: 2.232.
- Triângulos estimados: 4.356.
- Ossos: 18.
- Grupos deformadores: 17.
- Vértices ponderados e normalizados: 2.312 de 2.312.
- Actions: `Idle`, `RESET`, `Run`, `Walk`.
- Troca de Actions sem pose residual: aprovada.
- Proteção contra sobrescrita: aprovada por comparação SHA-256.

## Validação GLB e Godot

- GLB: 564.000 bytes.
- Malhas exportadas: corpo e espada.
- Câmeras exportadas: 0.
- Luzes exportadas: 0.
- Skeleton3D importados: 1.
- Ossos importados: 18.
- MeshInstance3D importados: 2.
- Animações importadas: `Idle`, `RESET`, `Run`, `Walk`.
- Compilação C#: 0 erros e 0 avisos.
- Cena AeriviaTest: 105 malhas, 22 colisões, câmera e escala aprovadas.
- O nó visual usa escala uniforme 0,95 para o cabelo de 1,892 m caber na
  cápsula física de 1,8 m; o arquivo Blender permanece em escala métrica real.
- Inicialização da cena principal/login: aprovada.

## Comandos principais

```powershell
& 'C:\Program Files\Blender Foundation\Blender 5.1\blender.exe' --background --factory-startup --python 'tools\blender\criar_personagem_principal.py' -- --aerivia-generate --version=v005

& 'C:\Program Files\Blender Foundation\Blender 5.1\blender.exe' '3D\blender\characters\Aerivia_MainCharacter_v005.blend' --background --python 'tools\blender\validar_personagem_principal.py'

& 'C:\Program Files\Blender Foundation\Blender 5.1\blender.exe' '3D\blender\characters\Aerivia_MainCharacter_v005.blend' --background --python 'tools\blender\exportar_personagem_principal_godot.py' -- --aerivia-export

dotnet build 'client\Weapons Masters Client.csproj' --nologo
```

## Limitações restantes

- O personagem continua sendo um blockout low-poly, não arte final.
- Os pesos são rígidos por segmento; não existe deformação orgânica nas juntas.
- Não há rig de controle/IK para trabalho de animação avançado.
- Idle, Walk e Run são ciclos provisórios.
- Ainda não existem animações de pulo, queda, ataque, dano ou morte.
- A integração atual é exclusiva da cena offline `AeriviaTest`; nenhum código de
  multiplayer foi modificado.
