# Arquitetura do ORDIS

```
ORDIS.exe (Rust + Tauri, interface HTML/CSS/JS no WebView2)
 ├─ agente ──────────► TabbyAPI (Python, exllamav3)  127.0.0.1:5000  ─► GPU
 │   └─ ferramentas: read, session_status, web_fetch,
 │      ordis_buscar_web (Python/ddgs), proxmox_consultar, ordis_gerar_imagem
 ├─ imagens ─────────► ComfyUI (Python)              127.0.0.1:8188  ─► GPU
 ├─ controle: inicia, encerra e acompanha os dois serviços
 └─ modelos: lê e grava o config.yml do TabbyAPI e o runtime\modelos.json
ordis-cli.exe: o mesmo núcleo no terminal, mais o benchmark
```

## Componentes

- **App** (`app/src`): `main.rs` expõe os comandos à interface; `agente.rs` monta o contexto, transmite a resposta em streaming e executa as ferramentas (`ferramentas.rs`); `controle.rs` sobe e derruba os serviços; `imagens.rs` orquestra o ComfyUI; `modelos.rs` cuida da configuração dos modelos; `proxmox.rs` é o cliente somente leitura; `benchmark.rs` é o avaliador; `win.rs` reúne as chamadas da API do Windows.
- **Interface** (`app/ui`): HTML, CSS e JS sem build. Tema claro e escuro, acrílico do Windows 11.
- **TabbyAPI**: servidor compatível com a API da OpenAI. O app fala com ele por `/v1/chat/completions` em streaming, com `reasoning_content` separado e chamadas de ferramenta.
- **ComfyUI**: recebe um fluxo pronto por `/prompt`; o progresso vem pelo WebSocket e o resultado por `/history`.

## GPU compartilhada

O modelo de conversa (~11 GB) e o de imagem (~11 GB no pico) não cabem juntos em 16 GB. Uma geração segura a trava de controle (`runtime\control.lock`) do começo ao fim: descarrega o modelo de conversa, sobe o ComfyUI, gera, encerra o ComfyUI e restaura o modelo de conversa se ele estava ligado. **Desligar** grava `runtime\images.cancel`, que cancela a geração em curso e impede a restauração.

## Estado e processos

`runtime\state.json` registra os processos iniciados pelo ORDIS com PID, horário de criação e linha de comando, para nunca encerrar um processo alheio que esteja usando as mesmas portas. O TabbyAPI é parado de forma ordenada por um arquivo de parada; se não responder, a árvore de processos é encerrada.

## Dados

| O quê | Onde |
|---|---|
| Conversas | `runtime\agente\sessoes\*.json` |
| Imagens geradas e parâmetros | `agente\workspace\imagens` |
| Anexos | `agente\workspace\anexos` |
| Prompt de sistema | `agente\workspace\AGENTS.md` + `AGENTS.local.md` (opcional, fora do Git) |
| Credencial do Proxmox | `runtime\proxmox` (DPAPI) |
| Configuração dos modelos | `runtime\tabbyAPI\config.yml`, `runtime\modelos.json`, `runtime\fluxos` |
| Métricas de tokens/s | `runtime\token-metrics.json` |
| Logs | `runtime\logs` |
