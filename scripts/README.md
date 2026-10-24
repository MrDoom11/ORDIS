# Scripts Python do ORDIS

O app em `app/` faz o controle, o agente, as imagens, o Proxmox e a avaliação do modelo. Esta pasta guarda só o que precisa continuar em Python. Instalações e dados ficam em `runtime/`, conforme `config/paths.json`.

- `serve_inference.py` e `service_shutdown.py`: inicialização e parada ordenada do TabbyAPI.
- `ordis_token_monitor.py` e `token_counter_paths.py`: métricas nativas de tokens/s gravadas em `runtime\token-metrics.json`, sem ler o texto das conversas. Rodam dentro do TabbyAPI.
- `agent_web_search.py`: pesquisa web do agente (ddgs), executada pelo Python de `runtime\tools-venv`.

Consulte [COMO USAR.md](../COMO%20USAR.md) e [docs/ARQUITETURA.md](../docs/ARQUITETURA.md).

## Travas do Git

`git-hooks/` tem duas travas, ativadas com `git config core.hooksPath scripts/git-hooks`:

- `pre-commit` recusa commits cujas linhas novas tenham IPs de rede privada, caminhos de usuário do Windows, chaves privadas ou tokens.
- `pre-push`, para o remoto `origin` (o repositório público), aceita qualquer branch, mas repete a verificação nos commits que o público ainda não tem e exige o e-mail `@users.noreply.github.com`. Outros remotos passam sem restrição.

Padrões pessoais (nome, e-mail, servidor) vão em `local/padroes-privados.txt`, um por linha. Essa pasta fica fora do Git, para que a própria trava não exponha esses dados.
