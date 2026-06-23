# Mapa de Arquitetura: Saint Seiya Online (Legacy) vs. MMORPG Moderno

Este documento mapeia a arquitetura original do jogo **Saint Seiya Online (SSO)** — baseada no framework e engine clássicos da **Perfect World Entertainment (PWE)** — e apresenta uma transposição desse modelo para práticas, tecnologias e arquiteturas **modernas** de MMORPG.

---

## 1. Arquitetura Clássica do Saint Seiya Online (SSO)

O SSO utiliza a clássica arquitetura cliente-servidor distribuída desenvolvida pela Beijing Perfect World. Essa infraestrutura foi projetada na década de 2000/2010 para lidar com milhares de conexões simultâneas usando processos 32-bit dedicados e comunicação baseada em RPC customizado.

```mermaid
graph TD
    %% Clientes e Conexão Externa
    Client("Cliente (elementclient.exe)<br>Angelica SDK 2.2 / DirectX 9")
    
    subgraph Servidor SSO (Distribuição TCP/RPC)
        glinkd("glinkd<br>(Gateway / Porta 29000)")
        gdeliveryd("gdeliveryd<br>(Coordenador Central / Sessões)")
        gamedbd("gamedbd<br>(Daemon de Banco de Dados)")
        gamed("gamed / gs<br>(Game Server / Loop Físico)")
        backdbd("backdbd<br>(Backup DB)")
        logservice("logservice<br>(Gerador de Logs)")
        gqueryd("gqueryd / queryd<br>(Status / GM Tools)")
    end

    %% Bancos de Dados
    BDB[("Berkeley DB<br>(Arquivos db4 / Key-Value)")]
    SQLDB[("SQL Database<br>(MySQL/MS SQL - Auth/Cash)")]

    %% Conexões
    Client <-- TCP / XOR Encryption --> glinkd
    glinkd <-->|TCP / RPC| gdeliveryd
    gdeliveryd <-->|TCP / RPC| gamed
    gamedbd <-->|Leitura/Escrita BDB| BDB
    gdeliveryd <-->|TCP / RPC| gamedbd
    backdbd -.->|Replicação/Hot Backup| BDB
    gdeliveryd <-->|Auth / Billing| SQLDB
    
    %% Logs & Consultas
    glinkd & gdeliveryd & gamed & gamedbd -.->|Logs TCP| logservice
    gqueryd <-->|Consultas| gdeliveryd
```

### Componentes Legados do Servidor
1. **`glinkd` (Gateway / Link Daemon):** O único ponto exposto à internet. Gerencia a criptografia (RC4/XOR), compressão de pacotes e encaminha a entrada do usuário para o respectivo servidor de mapa (`gs`).
2. **`gdeliveryd` (Delivery Daemon):** O cérebro do servidor. Gerencia chats globais, leilões, sistemas de guilda, lista de amigos, correios e validação de login. É quem faz a ponte entre o gateway e o banco de dados.
3. **`gamedbd` (Game Database Daemon):** Responsável por ler e salvar o progresso dos jogadores. Em vez de interagir com bancos de dados relacionais complexos em tempo real (o que causaria lentidão), ele armazena os dados dos jogadores compactados como blobs em arquivos **Berkeley DB (BDB)** de alta performance.
4. **`gamed / gs` (Game Server):** O loop principal do jogo (ticks, movimentação, colisão, IA de monstros e combate). Um servidor pode rodar múltiplos processos `gs` para instanciar mundos diferentes.
5. **`logservice`:** Recebe dumps de texto em rede dos outros daemons e grava arquivos de log consolidados.

---

## 2. Transposição para Metodologias Modernas

Se você planeja criar um MMORPG moderno inspirado nesta arquitetura, a separação funcional de responsabilidades (Gateway, Lógica de Jogo, Persistência, Coordenação Global) ainda é válida, mas as **tecnologias** mudaram drasticamente.

Abaixo, veja como traduzir cada componente clássico para pilares de sistemas modernos:

| Função Clássica | Componente SSO (Legado) | Abordagem Moderna | Vantagens da Abordagem Moderna |
| :--- | :--- | :--- | :--- |
| **Cliente / Engine** | Angelica Engine 2.2 / C++ / DX9 | **Unreal Engine 5** ou **Unity** | Ferramentas visuais robustas, iluminação realista (Lumen/Nanite), suporte multi-plataforma e física avançada. |
| **Gateway / Edge** | `glinkd` (TCP Customizado) | **Envoy Proxy**, **Agones Edge** ou **Gateways baseados em Go/Rust** | Alta tolerância a falhas, balanceamento automático de carga, terminação SSL automática, e mitigação nativa de DDoS. |
| **Coordenador Central** | `gdeliveryd` (Processo Monolítico C++) | **Microsserviços em NestJS / Go / gRPC** + **Redis** | Desacoplamento. Chat, Guildas, Leilões e Amigos viram microsserviços escaláveis independentes em nuvem. |
| **Protocolo de Rede** | XML + `rpcgen` customizado | **gRPC** ou **Protobuf / FlatBuffers** sobre **WebSockets/QUIC/UDP** | Compressão binária ultrarrápida, geração de código automática para qualquer linguagem de programação, e baixo consumo de banda. |
| **Persistência de Dados** | `gamedbd` + Berkeley DB (Blobs binários) | **MongoDB** ou **PostgreSQL** + Caching com **Redis** | Flexibilidade de esquemas (JSON no Mongo), facilidade de manutenção e indexação para análises em tempo real, sem corrupção de arquivos BDB. |
| **Game Server (Física/IA)** | `gs / gamed` (C++ Monolítico) | **Orquestração de Containers (Docker + Agones no Kubernetes)** | Escalabilidade elástica. Instâncias e servidores de mapa sobem e descem conforme a demanda de jogadores automaticamente. |
| **Logs e Auditoria** | `logservice` (Gravação local) | **Grafana Loki** + **Prometheus** (ou ELK Stack) | Monitoramento e telemetria em tempo real através de dashboards modernos, detecção preventiva de bugs e análise de economia do jogo. |

---

## 3. Arquitetura de MMORPG Moderno (Cloud-Native)

Esta é a estrutura recomendada para o seu novo MMORPG, utilizando conteinerização e microsserviços:

```mermaid
graph TD
    %% Camada de Cliente
    Client["Cliente de Jogo<br>(Unity / UE5)"]

    %% Camada de Entrada / Edge
    subgraph Camada de Rede (Edge)
        Gateway["API Gateway / Proxy<br>(Envoy / Traefik / Go-Edge)"]
        Agones["Agones / Kubernetes<br>(Gerenciador de Pods de Jogo)"]
    end

    %% Microsserviços e Coordenação
    subgraph Microsserviços de Apoio
        AuthSrv["Serviço de Auth / Conta<br>(Go / Node.js)"]
        SocialSrv["Serviço Social<br>(Amigos, Guildas, Chat)"]
        MatchSrv["Serviço Matchmaking<br>(Fila, Instâncias)"]
    end

    %% Camada de Cache e Eventos
    Redis[("Redis Cache<br>(Sessões & Estado Temporário)")]
    Kafka{{"Message Broker<br>(Kafka / NATS / RabbitMQ)"}}

    %% Instâncias Ativas de Jogo
    subgraph Cluster de Game Servers (Dedicated Pods)
        GS1["Game Server: Main World<br>(C++ Headless / C#)"]
        GS2["Game Server: Dungeon 01<br>(C++ Headless / C#)"]
    end

    %% Bancos de Dados
    Mongo[("MongoDB / DynamoDB<br>(Dados do Jogador / Inventário)")]
    Postgres[("PostgreSQL<br>(Contas, Logs Financeiros, Guildas)")]

    %% Fluxos de Rede
    Client -->|WebSocket / gRPC| Gateway
    Client -->|UDP / QUIC / RUDP| Agones
    
    Gateway --> AuthSrv & SocialSrv & MatchSrv
    Agones <-->|Alocação Dinâmica| GS1 & GS2

    %% Integração de microsserviços
    AuthSrv & SocialSrv & MatchSrv <--> Redis
    AuthSrv --> Postgres
    SocialSrv --> Mongo

    %% Eventos e Comunicação entre Servidores
    GS1 & GS2 <-->|gRPC / PubSub| Kafka
    SocialSrv <-->|Eventos| Kafka
    
    %% Persistência do Jogo
    GS1 & GS2 <-->|Salvar Estado| Mongo
```

### Explicação do Fluxo Moderno:
1. **Autenticação:** O cliente se conecta via HTTPS/gRPC ao microsserviço de **Auth**. Ao logar, a sessão é registrada no **Redis** e um token JWT é devolvido ao cliente.
2. **Entrada no Mundo:** O cliente solicita entrada. O serviço de **Matchmaking** conversa com o **Agones** (Kubernetes) para obter o IP/Porta de um **Game Server pod** disponível.
3. **Loop de Jogo:** O cliente conecta via UDP de baixa latência diretamente ao Game Server alocado. O Game Server lê os dados do jogador (inventário, atributos) diretamente do banco de dados **MongoDB** (carregando no Redis para acesso instantâneo).
4. **Interações Sociais:** Ações como mensagens de chat global, convites para guilda e trocas comerciais passam pelo **Message Broker (NATS/Kafka)**, permitindo que jogadores em diferentes instâncias (`GS1` e `GS2`) se comuniquem perfeitamente em tempo real, sem sobrecarregar o loop físico do jogo.

---

## 4. Recomendações para Iniciar seu Projeto

Se você está começando sozinho ou com uma equipe pequena, implementar toda a arquitetura Cloud-Native de imediato pode ser complexo. Siga estas etapas evolutivas:

1. **Comece Monolítico, mas modularizado:**
   Escreva seu servidor em **C# (compatível com Unity)** ou **C++ (compatível com Unreal)** em um formato modular. Isso facilita dividir o código em microsserviços no futuro se o jogo crescer.
2. **Evite TCP puro para o Loop de Jogo:**
   Use pacotes de rede modernos como **RiptideNetworking** (C#), **Mirror/FishNet** (Unity), ou a replicação nativa da **Unreal Engine** que utilizam UDP sob o capô para evitar problemas de perda de pacote trancando o fluxo (head-of-line blocking).
 3. **Use Banco de Dados NoSQL para Jogadores:**
   Armazenar um personagem de RPG (com dezenas de itens, quests ativas, árvore de talentos) é perfeito para estruturas de documento JSON. O **MongoDB** permitirá que você altere o formato dos itens do inventário no meio do desenvolvimento sem precisar rodar migrações de tabelas SQL complexas.
4. **Utilize o Docker desde o Dia 1:**
   Mantenha seu servidor empacotado em containers Docker. Isso garante que a transição para ambientes Kubernetes na nuvem (AWS/GCP) ocorra de maneira idêntica ao seu computador de desenvolvimento local.
