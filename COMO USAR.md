# Usar o ORDIS

Abra o atalho **ORDIS** (Área de Trabalho ou menu Iniciar). É um único app com quatro abas, na barra de ícones à esquerda:

- **Agente**: conversa com o modelo, com pesquisa web, leitura de arquivos do workspace, consultas ao Proxmox e geração de imagens.
- **Imagens**: envia um prompt diretamente ao gerador de imagens.
- **Painel**: liga e desliga os modelos e mostra serviços, memória da GPU e tokens/s.
- **Configurações**: tema, modelos de conversa e de imagem, MTP, conexão com o Proxmox e atalhos.

O modelo não liga sozinho com o Windows. A primeira mensagem ao agente o carrega (~30 s), ou use **Iniciar** no Painel ou no ponto de estado da barra lateral. Fechar a janela **não** desliga o modelo: use **Desligar** quando precisar da GPU para outra coisa.

## Agente

Escreva e tecle Enter; Shift+Enter quebra a linha. O raciocínio do modelo aparece recolhido, e cada ferramenta usada aparece como uma linha que pode ser aberta para ver argumentos e resultado. O botão de parar (ou Esc) interrompe a resposta na hora. Cada resposta tem um botão de copiar; a última tem também **Refazer**.

Para enviar uma imagem, use o clipe, cole com Ctrl+V ou arraste para a conversa. As conversas ficam na coluna da esquerda, agrupadas por data, com busca (Ctrl+K), renomear e apagar. Links das respostas abrem no navegador padrão.

O agente decide sozinho quando pesquisar. As consultas web saem deste computador para os provedores de pesquisa; não inclua credenciais, IPs internos ou logs privados nas perguntas que pedem pesquisa. As consultas ao Proxmox são somente de leitura.

O contexto padrão é de **32.768 tokens**, compartilhado entre instruções, histórico, raciocínio e resposta. Em conversas longas, os turnos mais antigos deixam de ser enviados ao modelo. Comece uma conversa nova ao mudar de assunto.

O comportamento do agente vem de `agente\workspace\AGENTS.md`. Para acrescentar algo pessoal (seu nome, preferências), crie `agente\workspace\AGENTS.local.md`; ele é lido junto e não vai para o Git.

## Imagens

Na aba **Imagens**, escreva o prompt (em inglês costuma funcionar melhor), escolha o formato e, se quiser, uma seed. Durante a geração, o modelo de conversa sai da GPU e volta no fim, se estava ligado; o ponto de estado pisca e o agente avisa enquanto isso. **Cancelar** interrompe só aquele pedido. Nas recentes, **Reutilizar** devolve prompt, formato e seed ao formulário.

Pelo agente, basta pedir a imagem; o modelo prepara o prompt e a imagem aparece na conversa.

## Modelos

Em **Configurações › Modelos** você troca o modelo de conversa (pastas em `runtime\tabbyAPI\models`) e o de imagem (arquivos em `runtime\ComfyUI\models\checkpoints`, ou um fluxo do ComfyUI importado), ajusta contexto, formato de ferramentas, raciocínio, visão e MTP, e usa **Testar conversa** e **Testar imagem** antes de depender do modelo novo. **Restaurar padrão** volta à configuração de fábrica.

## Terminal

`bin\ordis-cli.exe` faz o mesmo pelo terminal: `estado`, `iniciar`, `parar`, `perguntar "texto"`, `proxmox <consulta> [nó]`, `imagem "prompt"` e `benchmark`, a avaliação manual do modelo (reserve a GPU; leva horas no padrão completo).

## Dados e manutenção

- App instalado: `bin`. Para atualizar depois de mudar o código, feche o ORDIS e rode `app\instalar.ps1`.
- Conversas do agente: `runtime\agente\sessoes`.
- Imagens anexadas: `agente\workspace\anexos`. Imagens geradas e seus parâmetros: `agente\workspace\imagens`.
- Conexão do Proxmox: `runtime\proxmox` (segredo protegido pelo usuário do Windows).
- Configuração dos modelos: `runtime\tabbyAPI\config.yml`, `runtime\modelos.json` e `runtime\fluxos`.
- Instalações e pesos: `runtime`. Logs: `runtime\logs`.

Conversas, imagens e ajustes ficam só neste computador e não vão para o Git. Faça uma cópia externa dessas pastas de vez em quando; modelos e programas podem ser baixados de novo.

## Se houver erro

Veja o **Painel** e use **Logs**. Se uma porta estiver ocupada, o ORDIS informa o problema sem encerrar o programa alheio. Não compartilhe logs brutos: o log da inferência registra as chaves da API local.
