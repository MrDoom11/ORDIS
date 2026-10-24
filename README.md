# ORDIS

Assistente local para Windows que roda inteiro na sua GPU: conversa com um modelo de linguagem com visão, pesquisa na web, lê arquivos, consulta um servidor Proxmox em modo somente leitura e gera imagens, sem depender de serviços na nuvem.

É um único app em Rust + Tauri. O modelo de conversa é servido pelo [TabbyAPI](https://github.com/theroyallab/tabbyAPI) (exllamav3) e as imagens pelo [ComfyUI](https://github.com/comfyanonymous/ComfyUI). Como os dois não cabem juntos numa GPU de 16 GB, o ORDIS troca os modelos automaticamente quando você pede uma imagem e restaura o de conversa no fim.

## O que faz

- **Agente**: conversa com raciocínio visível, markdown e imagens. Tem ferramentas para ler arquivos do workspace, pesquisar na web, ler páginas, consultar o Proxmox e gerar imagens.
- **Imagens**: prompt direto ao gerador, com formato, seed, progresso real e cancelamento.
- **Painel**: liga e desliga os modelos e mostra memória da GPU e tokens/s.
- **Configurações**: tema claro, escuro ou automático; escolha dos modelos de conversa e de imagem, com teste; MTP (geração ~65% mais rápida em modelos compatíveis); conexão com o Proxmox.
- **Terminal**: `ordis-cli` faz o mesmo pela linha de comando, com um avaliador de qualidade e latência do modelo.

Detalhes de cada aba e das ferramentas em [app/README.md](app/README.md) e [COMO USAR.md](COMO%20USAR.md). Medições de memória, disco e tempo em [docs/MEDICOES-2026-10-23.md](docs/MEDICOES-2026-10-23.md).

## Requisitos

- Windows 11 com WebView2 (já vem no sistema).
- GPU NVIDIA com 16 GB de VRAM para a configuração padrão (testado numa RTX 5060 Ti). Com menos VRAM, escolha modelos menores em **Configurações › Modelos**.
- ~45 GB livres em disco e 32 GB de RAM.
- Rust (toolchain `stable-x86_64-pc-windows-gnu`, fixado em `app/rust-toolchain.toml`) para compilar.

## Estado do projeto

O app funciona e é usado no dia a dia, mas **ainda não há instalador**: o ambiente de execução (`runtime/`, com Python, TabbyAPI, ComfyUI e os modelos) é montado à mão. Um assistente de primeiro uso está nos planos.

### Montar o runtime

Tudo fica em `runtime/`, ao lado de `app/`, fora do Git (o caminho está em `config/paths.json`).

1. **Modelo de conversa**: Python 3.12, um venv em `runtime/tabbyAPI/venv` com PyTorch 2.9 (CUDA 12.8) e exllamav3 1.4.8, e o TabbyAPI (commit `de76ff8`) em `runtime/tabbyAPI`. A configuração usada está em `config/tabbyapi-inicial.yml` (copie para `runtime/tabbyAPI/config.yml`) e o preset de amostragem em `config/ordis_qwen_thinking.yml`. Pacotes exatos: `config/runtime-packages.txt`.
2. **Pesos**: [turboderp/Qwen3.8-27B-exl3](https://huggingface.co/turboderp/Qwen3.8-27B-exl3), variante 3.0 bpw, em `runtime/tabbyAPI/models/Qwen3.8-27B-exl3-3.0bpw`.
3. **Imagens**: ComfyUI 0.35 em `runtime/ComfyUI` com um venv próprio em `runtime/images-venv` (pacotes em `config/images-requirements.lock.txt`), e o checkpoint [RealVisXL V5.0 fp16](https://huggingface.co/SG161222/RealVisXL_V5.0) em `runtime/ComfyUI/models/checkpoints`.
4. **Pesquisa web**: um venv em `runtime/tools-venv` com o pacote `ddgs`.

### Compilar e instalar o app

```powershell
cd app
.\instalar.ps1
```

O script compila, copia `ORDIS.exe`, `ordis-cli.exe` e `WebView2Loader.dll` para `bin/` e cria atalhos na Área de Trabalho e no menu Iniciar.

## Estrutura

- `app/`: o app (Rust em `src/`, interface em `ui/`).
- `agente/workspace/`: área de trabalho do agente; `AGENTS.md` é o prompt de sistema. Para dados pessoais (seu nome, preferências), crie `AGENTS.local.md` ao lado: ele é lido junto e fica fora do Git.
- `config/`: configurações de referência, sem credenciais.
- `scripts/`: o que continua em Python (pesquisa web e a ponte de métricas do TabbyAPI).
- `docs/`: arquitetura, guia do Proxmox, medições e avaliação do modelo.

## Privacidade e segurança

- Os serviços escutam só em `127.0.0.1`. Conversas, imagens e credenciais ficam em `runtime/` e `agente/workspace/`, fora do Git.
- O acesso ao Proxmox é somente leitura (papel `PVEAuditor`), com o segredo cifrado pelo Windows (DPAPI) e TLS confiando só na CA do servidor. Veja [docs/PROXMOX.md](docs/PROXMOX.md).
- A pesquisa web envia os termos buscados a provedores externos (DuckDuckGo, Bing, Yahoo). A leitura de páginas recusa endereços locais e de rede privada.
- Os logs do TabbyAPI registram as chaves da API local: não compartilhe logs brutos.

## Licenças de terceiros

O ORDIS não inclui o código nem os pesos dos componentes abaixo; eles são instalados à parte, cada um com sua licença: TabbyAPI (AGPL-3.0), ComfyUI (GPL-3.0), exllamav3 (MIT), Qwen3.8 (Apache-2.0, segundo a página do modelo) e RealVisXL (consulte a página do modelo).
