# Avaliação local ORDIS — 2026-10-20

Estado: **completed**; execuções registradas: 52.

Mesmo Qwen3.8-27B EXL3 3.0 bpw; tarefas sintéticas representativas de auditoria. Nenhum comando ou consulta foi executado na infraestrutura.

| Contexto | Raciocínio | Acertos | Cortes | Mediana total (s) | Primeiro token (s) | Tokens/s | Pico amostrado VRAM (MiB) |
|---|---|---|---|---|---|---|---|
| 16384 | medium | 12/12 | 0 | 10.703 | 0.593 | 28.47 | 13127.0 |
| 16384 | xhigh | 12/12 | 0 | 10.265 | 0.609 | 28.41 | 13165.0 |
| 32768 | medium | 12/12 | 0 | 10.938 | 0.555 | 29.25 | 13727.0 |
| 32768 | xhigh | 12/12 | 0 | 12.25 | 0.594 | 29.15 | 13721.0 |

## Contexto longo adicional

O caso de aproximadamente 26 mil tokens é exclusivo de 32k e não entra nas medianas comparativas acima.

- medium, repetição 1: acerto, 51.328 s, 25952 tokens de entrada, pico 13576.0 MiB.
- xhigh, repetição 1: acerto, 64.969 s, 25991 tokens de entrada, pico 13574.0 MiB.
- medium, repetição 2: acerto, 48.891 s, 25952 tokens de entrada, pico 13579.0 MiB.
- xhigh, repetição 2: acerto, 55.219 s, 25991 tokens de entrada, pico 13580.0 MiB.

## Método e limites

- 2 repetições por caso e perfil; limite de saída 4096 tokens, incluindo raciocínio.
- Entrada curta e caso comum de aproximadamente 11 mil tokens em ambos os contextos. Prefixos distintos evitam reaproveitar integralmente prompts anteriores; tokens em cache também são registrados.
- Amostragem recomendada do modelo: temperatura 1, top_p 0,95, top_k 20; seeds fixadas por repetição.
- Primeiro token inclui raciocínio. Tempo até a resposta final é registrado separadamente no JSON.
- Memória total da GPU amostrada a cada 500 ms; inclui Windows e outros aplicativos, e pode não capturar um pico entre amostras.
- Os acertos usam gabaritos JSON exatos. Não medem toda a qualidade de diagnósticos abertos, visão ou confiabilidade das ferramentas do agente.
- Duas repetições dão uma comparação inicial, sem estabelecer significância estatística.
- Configuração original restaurada byte a byte: True; serviços encerrados: True.

## Evidências

- Resultados por caso, tokens, tempos, respostas finais, configurações e hashes: `BENCHMARK-2026-10-20.json`.
- Fonte dos níveis de raciocínio e da amostragem: https://huggingface.co/Qwen/Qwen3.8-27B
