// Configurações > Modelos: escolha do modelo de conversa e do de imagem,
// com teste e volta ao padrão.
(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const { el, icone, avisar } = window.ordis;

  let opcoes = null;
  let salvo = null; // JSON do que está gravado, para saber se há alterações
  let ocupado = false;

  const opcao = (valor, texto) => {
    const o = el('option', '', texto);
    o.value = valor;
    return o;
  };
  const gb = (n) => `${String(n).replace('.', ',')} GB`;

  function preencherListas() {
    $('mc-modelo').replaceChildren(
      ...opcoes.modelos_conversa.map((m) =>
        opcao(m.nome, `${m.nome} · ${gb(m.gb)}${m.visao ? ' · com visão' : ''}${m.mtp ? ' · MTP' : ''}`),
      ),
    );
    $('mc-ferramentas').replaceChildren(
      opcao('', 'Nenhum (sem ferramentas)'),
      ...opcoes.ferramentas.map((f) => opcao(f, f)),
    );
    $('mi-checkpoint').replaceChildren(...opcoes.checkpoints.map((m) => opcao(m.nome, `${m.nome} · ${gb(m.gb)}`)));
    $('mi-fluxo').replaceChildren(
      ...(opcoes.fluxos.length ? opcoes.fluxos.map((f) => opcao(f, f)) : [opcao('', 'Nenhum fluxo importado')]),
    );
    $('mi-sampler').replaceChildren(...opcoes.samplers.map((s) => opcao(s, s)));
    $('mi-scheduler').replaceChildren(...opcoes.schedulers.map((s) => opcao(s, s)));
  }

  // Garante que o valor atual exista na lista (ex.: contexto fora do padrão).
  function escolher(select, valor, texto) {
    if (![...select.options].some((o) => o.value === String(valor))) select.append(opcao(valor, texto || String(valor)));
    select.value = String(valor);
  }

  function mostrarFonte(fonte) {
    for (const b of document.querySelectorAll('#mi-fonte button')) b.classList.toggle('ativo', b.dataset.valor === fonte);
    $('mi-bloco-checkpoint').hidden = fonte !== 'checkpoint';
    $('mi-bloco-fluxo').hidden = fonte !== 'fluxo';
  }

  function preencherFormulario(conversa, imagem) {
    escolher($('mc-modelo'), conversa.modelo, `${conversa.modelo} · não encontrado`);
    escolher($('mc-contexto'), conversa.contexto, `${conversa.contexto} tokens`);
    $('mc-max').value = conversa.max_tokens;
    escolher($('mc-ferramentas'), conversa.ferramentas, conversa.ferramentas);
    $('mc-raciocinio').checked = conversa.raciocinio;
    $('mc-visao').checked = conversa.visao;
    $('mc-mtp').checked = conversa.mtp;
    $('mc-mtp-tokens').value = conversa.mtp_tokens;
    escolher($('mi-checkpoint'), imagem.checkpoint, `${imagem.checkpoint} · não encontrado`);
    if (imagem.fluxo) escolher($('mi-fluxo'), imagem.fluxo, imagem.fluxo);
    $('mi-passos').value = imagem.passos;
    $('mi-cfg').value = imagem.cfg;
    escolher($('mi-sampler'), imagem.sampler);
    escolher($('mi-scheduler'), imagem.scheduler);
    mostrarFonte(imagem.fluxo ? 'fluxo' : 'checkpoint');
    avaliar();
  }

  function lerFormulario() {
    const fluxo = document.querySelector('#mi-fonte button.ativo')?.dataset.valor === 'fluxo';
    return {
      conversa: {
        modelo: $('mc-modelo').value,
        contexto: Number($('mc-contexto').value),
        max_tokens: Number($('mc-max').value),
        ferramentas: $('mc-ferramentas').value,
        raciocinio: $('mc-raciocinio').checked,
        visao: $('mc-visao').checked,
        mtp: $('mc-mtp').checked,
        mtp_tokens: Number($('mc-mtp-tokens').value),
      },
      imagem: {
        checkpoint: $('mi-checkpoint').value,
        passos: Number($('mi-passos').value),
        cfg: Number($('mi-cfg').value),
        sampler: $('mi-sampler').value,
        scheduler: $('mi-scheduler').value,
        fluxo: fluxo ? $('mi-fluxo').value : '',
      },
    };
  }

  // Avisos de tamanho e estado dos botões conforme o formulário.
  function avaliar() {
    if (!opcoes) return;
    const f = lerFormulario();
    const vram = opcoes.vram_gb;
    const llm = opcoes.modelos_conversa.find((m) => m.nome === f.conversa.modelo);
    let aviso = '';
    if (!llm) aviso = 'Pasta não encontrada em runtime\\tabbyAPI\\models.';
    else if (vram && llm.gb > vram - 2.5)
      aviso = `Com ${gb(llm.gb)}, este modelo provavelmente não cabe nos ${gb(vram)} da GPU junto com o contexto.`;
    else if (llm && f.conversa.visao && !llm.visao) aviso = 'Este modelo não parece ter visão; a leitura de imagens deve falhar.';
    else if (llm && f.conversa.mtp && !llm.mtp) aviso = 'Este modelo não tem camada de MTP; o modelo não deve carregar com o MTP ligado.';
    else if (llm && vram && f.conversa.mtp && llm.gb + 1 > vram - 2.5)
      aviso = 'Com o MTP ligado (~1 GB a mais), este modelo pode não caber na GPU junto com o contexto.';
    $('mc-aviso').textContent = aviso;
    $('mc-aviso').hidden = !aviso;

    const ck = opcoes.checkpoints.find((m) => m.nome === f.imagem.checkpoint);
    let avisoImg = '';
    if (!f.imagem.fluxo && !ck) avisoImg = 'Arquivo não encontrado em runtime\\ComfyUI\\models\\checkpoints.';
    else if (!f.imagem.fluxo && vram && ck.gb > vram - 1)
      avisoImg = `Com ${gb(ck.gb)}, este modelo provavelmente não cabe nos ${gb(vram)} da GPU.`;
    $('mi-aviso').textContent = avisoImg;
    $('mi-aviso').hidden = !avisoImg;

    const alterado = JSON.stringify(f) !== salvo;
    const padrao = JSON.stringify(f) === JSON.stringify(opcoes.padrao);
    $('mo-salvar').disabled = ocupado || !alterado;
    $('mo-padrao').disabled = ocupado || padrao;
    $('mc-mtp-tokens').disabled = ocupado || !f.conversa.mtp;
    for (const id of ['mo-testar-conversa', 'mo-testar-imagem']) {
      $(id).disabled = ocupado || alterado;
      $(id).dataset.dica = alterado ? 'Salve antes de testar' : '';
      if (!alterado) delete $(id).dataset.dica;
    }
  }

  async function carregar(manter) {
    opcoes = await invoke('modelos_opcoes');
    const atual = manter ? lerFormulario() : null;
    preencherListas();
    salvo = JSON.stringify({ conversa: opcoes.conversa, imagem: opcoes.imagem });
    if (atual) preencherFormulario(atual.conversa, atual.imagem);
    else preencherFormulario(opcoes.conversa, opcoes.imagem);
  }

  function definirOcupado(sim, texto = '') {
    ocupado = sim;
    $('mo-estado').textContent = texto;
    for (const e of document.querySelectorAll('#secao-modelos input, #secao-modelos select, #secao-modelos #mi-fonte button')) {
      e.disabled = sim;
    }
    avaliar();
  }

  async function salvar(dados) {
    if (ocupado) return;
    definirOcupado(true, 'Salvando…');
    try {
      const r = await invoke('modelos_salvar', dados);
      await carregar(false);
      avisar(
        r.reiniciado
          ? 'Modelo de conversa trocado e recarregado.'
          : r.mudou_conversa
            ? 'Salvo. O novo modelo de conversa vale na próxima vez que ele carregar.'
            : 'Configuração salva.',
      );
      $('mo-resultado').hidden = true;
    } catch (e) {
      avisar(String(e), true);
    } finally {
      definirOcupado(false);
    }
  }

  function mostrarEtapas(etapas, imagemDataUrl) {
    const r = $('mo-resultado');
    r.replaceChildren(
      ...etapas.map((e) => {
        const linha = el('div', `etapa${e.ok ? '' : ' falhou'}`);
        linha.append(icone(e.ok ? 'i-ok' : 'i-x'), el('span', 'nome', e.nome), el('span', 'detalhe', e.detalhe), el('span', 'tempo', `${String(e.segundos).replace('.', ',')} s`));
        return linha;
      }),
    );
    if (imagemDataUrl) {
      const img = el('img');
      img.src = imagemDataUrl;
      img.addEventListener('click', () => {
        const z = el('div', 'zoom');
        const g = el('img');
        g.src = img.src;
        z.append(g);
        z.addEventListener('click', () => z.remove());
        document.body.append(z);
      });
      r.append(img);
    }
    r.hidden = false;
  }

  async function testarConversa() {
    definirOcupado(true, 'Testando o modelo de conversa…');
    try {
      const r = await invoke('modelos_testar_conversa');
      mostrarEtapas(r.etapas);
      avisar(r.ok ? 'O modelo de conversa passou no teste.' : 'O modelo de conversa falhou em parte do teste.', !r.ok);
    } catch (e) {
      avisar(String(e), true);
    } finally {
      definirOcupado(false);
    }
  }

  async function testarImagem() {
    definirOcupado(true, 'Gerando uma imagem de teste (troca de modelos na GPU, leva 1 a 2 minutos)…');
    window.ordis.definirTroca(true);
    const inicio = Date.now();
    try {
      const r = await invoke('gerar_imagem', {
        parametros: { prompt: 'a red apple on a white table, studio photo', steps: Math.min(12, Number($('mi-passos').value) || 12) },
      });
      const segundos = Math.round((Date.now() - inicio) / 100) / 10;
      if (r.ok) {
        const url = await invoke('imagem', { caminho: r.file });
        mostrarEtapas([{ nome: 'Gerar imagem', ok: true, detalhe: `${r.model}, ${r.width} × ${r.height}, seed ${r.seed}.`, segundos }], url);
        avisar('O modelo de imagem passou no teste.');
      } else {
        mostrarEtapas([{ nome: 'Gerar imagem', ok: false, detalhe: r.error || 'Falhou.', segundos }]);
        avisar('O modelo de imagem falhou no teste.', true);
      }
    } catch (e) {
      avisar(String(e), true);
    } finally {
      window.ordis.definirTroca(false);
      definirOcupado(false);
    }
  }

  listen('progresso', (ev) => {
    if (ocupado) $('mo-estado').textContent = ev.payload;
  });
  listen('imagem', ({ payload: s }) => {
    if (ocupado && s.message) $('mo-estado').textContent = s.message.replace(/\.\.\.$/, '…');
  });

  for (const id of ['mc-modelo', 'mc-contexto', 'mc-max', 'mc-ferramentas', 'mc-raciocinio', 'mc-visao', 'mc-mtp', 'mc-mtp-tokens', 'mi-checkpoint', 'mi-fluxo', 'mi-passos', 'mi-cfg', 'mi-sampler', 'mi-scheduler']) {
    $(id).addEventListener('input', avaliar);
    $(id).addEventListener('change', avaliar);
  }
  // Ao escolher outro modelo de conversa, a visão acompanha o que a pasta indica.
  $('mc-modelo').addEventListener('change', () => {
    const m = opcoes.modelos_conversa.find((x) => x.nome === $('mc-modelo').value);
    if (m) $('mc-visao').checked = m.visao;
    if (m && !m.mtp) $('mc-mtp').checked = false;
    avaliar();
  });
  for (const b of document.querySelectorAll('#mi-fonte button')) {
    b.addEventListener('click', () => {
      mostrarFonte(b.dataset.valor);
      avaliar();
    });
  }
  for (const b of document.querySelectorAll('[data-pasta]')) {
    b.addEventListener('click', () => invoke('modelos_abrir_pasta', { qual: b.dataset.pasta }).catch((e) => avisar(String(e), true)));
  }
  $('mi-importar').addEventListener('click', () => $('mi-arquivo-fluxo').click());
  $('mi-arquivo-fluxo').addEventListener('change', async (e) => {
    const f = e.target.files[0];
    e.target.value = '';
    if (!f) return;
    try {
      const nome = await invoke('modelos_importar_fluxo', { nome: f.name, conteudo: await f.text() });
      await carregar(true);
      escolher($('mi-fluxo'), nome);
      avaliar();
      avisar(`Fluxo "${nome}" importado. Salve para usá-lo.`);
    } catch (err) {
      avisar(String(err), true);
    }
  });
  $('mo-salvar').addEventListener('click', () => salvar(lerFormulario()));
  $('mo-padrao').addEventListener('click', () => {
    preencherFormulario(opcoes.padrao.conversa, opcoes.padrao.imagem);
    avisar('Valores de fábrica preenchidos. Salve para aplicar.');
  });
  $('mo-testar-conversa').addEventListener('click', testarConversa);
  $('mo-testar-imagem').addEventListener('click', testarImagem);
  window.addEventListener('aba', (e) => {
    if (e.detail === 'config' && !ocupado) carregar(false).catch((err) => avisar(String(err), true));
  });
})();
