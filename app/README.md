# App ORDIS (Rust + Tauri)

Aplicativo único do ORDIS. Substituiu os atalhos, os `.cmd`, o Open WebUI e o OpenClaw. A interface é HTML/CSS/JS puro em `ui/`, renderizada pelo WebView2 do Windows; não há Node nem etapa de build do frontend.

## Instalar e atualizar

```powershell
cd <pasta do ORDIS>\app
.\instalar.ps1
```

O script compila em release, copia `ORDIS.exe`, `ordis-cli.exe` e `WebView2Loader.dll` para `bin\` (na pasta do projeto) e recria o atalho **ORDIS** na Área de Trabalho e no menu Iniciar. Feche o app antes.

O `rust-toolchain.toml` seleciona o toolchain GNU, que não depende do Windows SDK. Por isso o WebView2 é carregado por `WebView2Loader.dll`, que precisa ficar ao lado do executável. Compile pelo PowerShell: no Git Bash, o `link` do Git pode interferir.

## Executáveis

- `ORDIS.exe`: a janela.
- `ordis-cli.exe estado|iniciar|parar|perguntar "texto"|proxmox <consulta> [nó]|imagem "prompt"|benchmark [opções]`: o mesmo controlador, agente, Proxmox, gerador e avaliação no terminal. É um executável de console separado porque o terminal não espera por um app de janela.

## Interface

A janela usa o acrílico do Windows 11. A navegação fica numa barra fina de ícones à esquerda, com o tema, o estado dos serviços e as configurações embaixo. O ponto de estado fica azul com o Qwen pronto e pisca enquanto inicia; um clique nele mostra os serviços, a memória da GPU e o botão de iniciar ou desligar. Durante uma geração de imagem, ele também pisca e a linha sob a caixa de mensagem avisa que o Qwen saiu da GPU e volta sozinho, para a troca de modelos não parecer travamento.

O tema pode ser Sistema (segue o modo claro ou escuro do Windows), Claro ou Escuro, em Configurações ou pelo ícone da barra. A escolha vale também para a barra de título e fica guardada no perfil do WebView2.

Atalhos: `Ctrl+1` a `Ctrl+4` trocam de aba, `Ctrl+N` abre uma conversa nova, `Ctrl+B` mostra ou esconde a lista de conversas, `Ctrl+K` busca conversas, `Esc` interrompe a resposta e `Ctrl+Enter` gera a imagem na aba Imagens.

## Abas

### Agente

Conversa com o Qwen pelo TabbyAPI, com raciocínio recolhível, markdown, imagens e o botão de parar, que interrompe a geração no Tabby. Se o modelo estiver desligado, o primeiro envio o carrega; a linha sob a caixa de mensagem diz se o Qwen está pronto.

- Conversas agrupadas por data, com busca, renomear (lápis ou duplo clique) e apagar com segundo clique de confirmação.
- Cada resposta tem botão de copiar; a última tem também **Refazer**, que descarta a resposta e pergunta de novo. Blocos de código têm botão de copiar próprio.
- Imagens podem ser anexadas pelo clipe, coladas ou arrastadas para a conversa.

- Prompt de sistema: `agente\workspace\AGENTS.md`, mais a data e a hora atuais.
- Conversas: `runtime\agente\sessoes\*.json`. Imagens anexadas: `agente\workspace\anexos`.
- Contexto: 32.768 tokens, com até 4.096 por resposta. Turnos antigos saem da janela por estimativa (3 caracteres por token); só as imagens do turno atual são enviadas ao modelo.
- Ferramentas, com os mesmos nomes que o OpenClaw usava:

| Ferramenta | O que faz |
|---|---|
| `read` | Lê arquivos e lista pastas do workspace; imagens vão para análise visual. Caminhos fora do workspace são recusados. |
| `session_status` | Data, hora, fuso, modelo e tamanho da conversa. |
| `web_fetch` | Baixa páginas públicas como texto. Recusa endereços locais e de rede privada, inclusive em redirecionamentos. |
| `ordis_buscar_web` | `scripts/agent_web_search.py` (ddgs) pelo Python de `runtime\tools-venv`. |
| `proxmox_consultar` | `src/proxmox.rs`, somente leitura: versão, nós, VMs, armazenamento, estado do nó, backups e discos por slot. |
| `ordis_gerar_imagem` | `src/imagens.rs`, com o progresso na linha de estado; Parar cancela o pedido. |

### Imagens

Prompt direto ao RealVisXL, com prompt negativo, formato e seed (o dado sorteia uma). Mostra a etapa, os passos reais e o tempo; **Cancelar** interrompe só aquele pedido. A galeria lista os PNGs de `agente\workspace\imagens`; ao escolher uma, a legenda mostra formato, seed e prompt, e **Reutilizar** devolve esses parâmetros ao formulário.

### Painel

Estado da inferência e do gerador de imagens, memória da GPU e tokens/s (de `runtime\token-metrics.json`). Um só botão alterna entre **Iniciar**, que carrega o Qwen, e **Desligar**, que encerra os serviços e libera a GPU.

### Configurações

**Modelos.** Escolhe o modelo de conversa e o de imagem sem recompilar:

- Conversa: a pasta do modelo em `runtime	abbyAPI\models` (formato EXL3 ou EXL2), contexto, limite por resposta, formato de ferramentas (a lista vem do próprio Tabby), raciocínio e visão. Esses valores vivem no `config.yml` do Tabby; o app só reescreve as linhas `model_name`, `max_seq_len`, `cache_size`, `tool_format`, `reasoning` e `vision`, preservando o resto. Se o modelo estiver carregado, **Salvar** o recarrega na hora.
- Imagem: um arquivo de `runtime\ComfyUI\models\checkpoints` com o fluxo SDXL embutido, ou um fluxo do ComfyUI exportado em formato API (Export (API)) e importado pelo botão **+**. No fluxo, o texto `{{prompt}}` marca o prompt positivo; também valem `{{negativo}}`, `{{seed}}`, `{{largura}}`, `{{altura}}`, `{{passos}}`, `{{cfg}}`, `{{sampler}}`, `{{scheduler}}` e `{{checkpoint}}`. Fluxos ficam em `runtime\fluxos`.
- MTP (Multi-Token Prediction): para modelos com a camada de MTP (o Qwen3.8-27B tem), o interruptor grava `draft_mode: mtp` e `draft_num_tokens` na seção `draft_model` do `config.yml`; desligar remove a seção. Medido aqui: geração de 30,3 para ~50 tokens/s com 3 tokens adiantados, ~1 GB a mais de VRAM, sem mudar o tempo de carga nem o processamento do prompt. Vem desligado de fábrica.
- Passos, cfg, sampler, scheduler e o limite por resposta ficam em `runtime\modelos.json`. Sem esse arquivo valem os padrões de fábrica (Qwen3.8-27B e RealVisXL V5).
- **Testar conversa** carrega o modelo e confere resposta curta, raciocínio separado, chamada de ferramenta e, com visão ligada, a leitura de uma imagem. **Testar imagem** gera uma imagem de 12 passos. **Restaurar padrão** preenche os valores de fábrica.
- A tela avisa quando o modelo parece não caber na GPU (pelo tamanho dos arquivos) ou quando a pasta não indica visão.

Também ficam aqui o tema, a lista de atalhos e a conexão somente leitura com o Proxmox: endereço, ID do token, segredo e CA. O segredo é cifrado com DPAPI para o usuário Windows e gravado em `runtime\proxmox`. As consultas usam HTTPS confiando só na CA do servidor, sem redirecionamentos e com até 50 chamadas por consulta.

## Imagens: como a GPU é dividida

`src/imagens.rs` segura a trava de controle durante toda a geração: descarrega o Qwen, sobe o ComfyUI, envia o workflow do RealVisXL, acompanha os passos pelo WebSocket do ComfyUI, encerra o ComfyUI e restaura o Qwen se ele estava ligado. **Cancelar** interrompe só aquele pedido; **Desligar** grava `runtime\images.cancel`, que cancela a geração e impede a restauração. Cada PNG ganha um `.json` com os parâmetros e o workflow.

## Avaliação do modelo

`ordis-cli benchmark [--contexts 16384 32768] [--efforts medium xhigh] [--repeats 2] [--max-tokens 4096] [--run-label R] [--report caminho.json]` roda tarefas sintéticas com gabarito JSON, mede tempo até o primeiro token e até a resposta, tokens/s e pico de VRAM, e grava o relatório JSON e Markdown (padrão: `docs\BENCHMARK-<rótulo>`). Ele exige os serviços desligados, segura a trava de controle até o fim, ajusta só o contexto no `config.yml` do Tabby e restaura o arquivo byte a byte. O Desligar do painel cancela a avaliação.

## Python que continua

- `scripts/serve_inference.py`, `service_shutdown.py`, `ordis_token_monitor.py` e `token_counter_paths.py` rodam dentro do TabbyAPI, que é Python.
- `scripts/agent_web_search.py` usa a biblioteca `ddgs` pelo Python de `runtime\tools-venv`. A ddgs imita um navegador para obter resultados reais; uma leitura direta das páginas do Bing e do DuckDuckGo em Rust foi testada e devolveu resultados degradados.

## Próximas fases

1. Cliente MCP.
2. Ações no PC e SSH com aprovação.
