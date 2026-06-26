---
name: wm-infra-ops
description: Infraestrutura, DevOps e observabilidade para Weapons Masters. Use para Docker Compose, Kubernetes, Gateway WebTransport/QUIC com portas UDP, WebSocket fallback TCP, PostgreSQL, Redis, NATS JetStream, Prometheus, Grafana, Loki, CI/CD, deploy cloud, backup, load balancing, TLS, secrets, migrations, ambientes dev/staging/prod e operacao de alpha publico.
---

# Weapons Masters Infra Ops

Use esta skill para tornar o jogo operavel. MMORPG que nao mede, nao recupera e nao faz backup vira aposta.

## Workflow

1. Identificar ambiente: local dev, staging, alpha ou producao.
2. Separar configuracao de codigo: `.env`, secrets, ports, volumes e migrations.
3. Garantir startup reproduzivel: compose sobe, migrations rodam, healthchecks passam.
4. Adicionar metricas e logs antes de load test.
5. Documentar comando de rollback, backup e restore quando mexer em dados.

## Stack De Referencia

- Dev local: Docker Compose com server, PostgreSQL, Redis e NATS.
- Gateway: WebTransport/QUIC em UDP e WebSocket fallback em TCP.
- Mensageria: NATS JetStream com stream duravel e consumers com ACK explicito.
- DB: PostgreSQL como fonte ACID de conta, personagem, inventario e economia.
- Cache/sessao: Redis, nunca fonte de verdade de item.
- Observabilidade: Prometheus para metricas, Grafana para dashboards, Loki ou logs estruturados.

## Rede E Portas

- Ao gerar Docker Compose, Kubernetes, Terraform, firewall, security group ou load balancer para o Gateway, expor explicitamente a porta UDP do WebTransport/QUIC, por exemplo `4433/udp`.
- Expor tambem a porta TCP do fallback WebSocket, por exemplo `8080/tcp`.
- Nao assumir que publicar `4433:4433` cobre UDP em Docker; usar sintaxe com protocolo: `4433:4433/udp`.
- Em Kubernetes, declarar `protocol: UDP` no `Service` do WebTransport e `protocol: TCP` no fallback.
- Em cloud, validar que o load balancer suporta UDP/QUIC; ALB HTTP tradicional nao substitui L4 UDP.
- Healthchecks TCP nao provam que WebTransport funciona; incluir smoke test ou metrica para handshake/datagram QUIC.
- Se TLS/certificado self-signed for usado em dev, documentar fingerprint/hash esperado para o cliente.

## Metricas Obrigatorias

- Tick real do World Server, p50/p95/p99.
- Tempo de drain de input por tick.
- Conexoes por Gateway e reconexoes por minuto.
- Timeouts de heartbeat, conexoes half-open e entidades removidas por inatividade.
- Bytes/s de snapshot e tamanho medio do payload.
- Fila pendente JetStream e falhas de ACK.
- Latencia DB worker e numero de retries.
- Login success/fail/rate-limited.
- Gold generated/removed, trades/min e loot raro/min.

## Regras De Producao

- Secrets nunca ficam hardcoded.
- Migrations sao versionadas e reversiveis quando possivel.
- Backup automatico antes de deploy que altera schema ou economia.
- Healthcheck nao e "processo esta vivo"; deve testar dependencia critica.
- Load balancer precisa preservar afinidade quando a sessao do transporte exigir.
- Deploy do Gateway so esta pronto quando UDP WebTransport e TCP fallback foram testados de fora do container/cluster.
- Configurar timeouts de load balancer/proxy de modo compativel com heartbeat do Gateway; nao deixar conexao half-open sobreviver sem limite.
