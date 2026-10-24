# Configurações de referência

Configurações versionadas, sem credenciais. Os dados privados e as instalações ficam em `runtime/`, fora do Git.

- `paths.json`: local do runtime. O padrão, `"runtime"`, é relativo à pasta do projeto; um caminho absoluto também funciona.
- `tabbyapi-inicial.yml`: configuração do TabbyAPI usada pelo ORDIS. Copie para `runtime/tabbyAPI/config.yml` ao montar o runtime; depois, o app a ajusta em **Configurações › Modelos**.
- `ordis_qwen_thinking.yml`: preset de amostragem do Qwen com raciocínio (vai em `runtime/tabbyAPI/sampler_overrides/`).
- `runtime-packages.txt` e `images-requirements.lock.txt`: pacotes exatos dos ambientes Python do TabbyAPI e do ComfyUI.

O prompt de sistema do agente fica em `agente/workspace/AGENTS.md`, e a conexão do Proxmox é configurada pelo app, em `runtime/proxmox`.
