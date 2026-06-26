# Source notes 2026

Use estas notas para decisoes recorrentes. Se a tarefa depender de versao exata, verificar documentacao oficial novamente.

## Godot 4 C# e Web

A documentacao oficial de export Web do Godot informa que projetos Godot 4 escritos em C# atualmente nao podem ser exportados para Web. Implicacao para Weapons Masters: nao prometer PC + Browser com um unico cliente Godot C# ate que a documentacao oficial mude. Alternativas: cliente Web separado, camada web em GDScript, build nativo C# primeiro, ou aguardar suporte oficial.

## Bevy ECS standalone

`bevy_ecs` pode ser usado como crate standalone. Isso combina com World Server Rust: componentes como dados, sistemas como funcoes, recursos para indices/filas e testes unitarios sem carregar engine completa.

## NATS JetStream

JetStream usa streams, consumers e ack explicito para entrega duravel/at-least-once. Handlers de persistencia precisam tolerar reentrega por idempotencia, dedupe key ou UPSERT transacional.
