(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const { el, avisar } = window.ordis;

  const ETAPAS = {
    preparing: 'Preparando a geração…',
    releasing_llm: 'Liberando a GPU para a imagem…',
    starting_images: 'Carregando o modelo de imagem…',
    generating: 'Gerando imagem…',
    cancelling: 'Cancelando a geração…',
    stopping_images: 'Finalizando e liberando a GPU…',
    restoring_llm: 'Restaurando o modelo de conversa…',
    completed: 'Imagem pronta. Finalizando…',
    cancelled: 'Geração cancelada. Finalizando…',
    failed: 'Finalizando após uma falha…',
  };
  const aviso = () => `Ao gerar, o ${window.ordis.nomes().conversa} sai da GPU e volta no fim. Leva alguns minutos.`;

  let gerando = false;
  let inicio = 0;
  let ultimaEtapa = 'preparing';
  let ultimoTexto = '';
  let relogio = null;
  let exibida = null; // { file, parameters }
  const cache = new Map();

  const tempo = (s) => {
    s = Math.max(0, Math.floor(s));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const ss = String(s % 60).padStart(2, '0');
    return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${String(m).padStart(2, '0')}:${ss}`;
  };

  async function dataUrl(caminho) {
    if (!cache.has(caminho)) cache.set(caminho, await invoke('imagem', { caminho }));
    return cache.get(caminho);
  }

  function legenda(p) {
    if (!p) return '';
    const partes = [`${p.width} × ${p.height}`, `seed ${p.seed}`];
    return `${partes.join(' · ')}${p.prompt ? ` — ${p.prompt}` : ''}`;
  }

  async function mostrar(item) {
    const img = el('img');
    img.src = await dataUrl(item.file);
    img.alt = item.file.split(/[\\/]/).pop();
    img.addEventListener('click', () => {
      const z = el('div', 'zoom');
      const grande = el('img');
      grande.src = img.src;
      z.append(grande);
      z.addEventListener('click', () => z.remove());
      document.body.append(z);
    });
    exibida = item;
    $('img-visor').replaceChildren(img);
    $('img-legenda').textContent = legenda(item.parameters) || img.alt;
    $('img-legenda').title = item.parameters?.prompt || '';
    $('img-acoes-visor').hidden = false;
    $('img-reutilizar').disabled = !item.parameters;
    for (const g of document.querySelectorAll('#img-galeria img')) g.classList.toggle('ativa', g.dataset.file === item.file);
  }

  async function galeria() {
    const lista = await invoke('imagens_recentes');
    const g = $('img-galeria');
    g.replaceChildren();
    if (!lista.length) {
      g.append(el('p', 'sutil pequeno', 'Nenhuma imagem gerada ainda.'));
      return;
    }
    for (const item of lista.slice(0, 30)) {
      const img = el('img');
      img.alt = item.file.split(/[\\/]/).pop();
      img.dataset.file = item.file;
      img.loading = 'lazy';
      if (exibida && exibida.file === item.file) img.classList.add('ativa');
      img.addEventListener('click', () => mostrar(item));
      g.append(img);
      dataUrl(item.file)
        .then((u) => (img.src = u))
        .catch(() => img.remove());
    }
  }

  function formato() {
    return document.querySelector('#img-formato button.ativo').dataset.valor;
  }

  function escolherFormato(valor) {
    for (const b of document.querySelectorAll('#img-formato button')) b.classList.toggle('ativo', b.dataset.valor === valor);
  }

  function reutilizar() {
    const p = exibida && exibida.parameters;
    if (!p) return;
    $('img-prompt').value = p.prompt || '';
    $('img-negativo').value = p.negative || '';
    $('img-seed').value = p.seed ?? '';
    const valor = `${p.width}x${p.height}`;
    if (document.querySelector(`#img-formato button[data-valor="${valor}"]`)) escolherFormato(valor);
    $('img-prompt').focus();
    avisar('Prompt, formato e seed copiados para o formulário.');
  }

  function atualizarEstado() {
    const base = ETAPAS[ultimaEtapa] || ultimaEtapa || 'Preparando a geração…';
    $('img-estado').textContent = `${base}${ultimoTexto ? ` · ${ultimoTexto}` : ''} · ${tempo((Date.now() - inicio) / 1000)}`;
  }

  function ocupado(sim) {
    gerando = sim;
    window.ordis.definirTroca(sim);
    $('img-gerar').disabled = sim;
    $('img-gerar').textContent = sim ? 'Gerando…' : 'Gerar imagem';
    $('img-cancelar').hidden = !sim;
    $('img-cancelar').disabled = !sim;
    $('img-progresso').hidden = !sim;
    for (const id of ['img-prompt', 'img-negativo', 'img-seed', 'img-sortear']) $(id).disabled = sim;
    for (const b of document.querySelectorAll('#img-formato button')) b.disabled = sim;
    $('img-progresso').classList.toggle('indeterminada', sim);
    if (!sim) $('img-barra').style.width = '0';
  }

  async function gerar() {
    if (gerando) return;
    const prompt = $('img-prompt').value;
    if (!prompt.trim()) {
      $('img-prompt').focus();
      avisar('Escreva um prompt.', true);
      return;
    }
    const [largura, altura] = formato().split('x').map(Number);
    const parametros = { prompt, negative: $('img-negativo').value, width: largura, height: altura };
    const seed = $('img-seed').value.trim();
    if (seed) {
      if (!/^\d+$/.test(seed) || Number(seed) > 4294967295) {
        avisar('A seed deve ser um inteiro entre 0 e 4294967295.', true);
        return;
      }
      parametros.seed = Number(seed);
    }
    ocupado(true);
    inicio = Date.now();
    ultimaEtapa = 'preparing';
    ultimoTexto = '';
    atualizarEstado();
    relogio = setInterval(atualizarEstado, 1000);
    try {
      const r = await invoke('gerar_imagem', { parametros });
      if (r.cancelled) {
        $('img-estado').textContent = `Geração cancelada após ${tempo((Date.now() - inicio) / 1000)}.`;
      } else if (!r.ok) {
        $('img-estado').textContent = aviso();
        avisar(`Falha: ${r.error || 'a geração não retornou um resultado utilizável.'}`, true);
      } else {
        await mostrar({
          file: r.file,
          parameters: { ...parametros, seed: r.seed, width: r.width, height: r.height },
        });
        $('img-estado').textContent = `Concluída em ${r.seconds} s.${r.warning ? ` ${r.warning}` : ''}`;
        avisar('Imagem pronta.');
        galeria();
      }
    } catch (e) {
      $('img-estado').textContent = aviso();
      avisar(`Falha: ${e}`, true);
    } finally {
      clearInterval(relogio);
      ocupado(false);
    }
  }

  listen('imagem', ({ payload: s }) => {
    if (!gerando) return;
    ultimaEtapa = s.stage || ultimaEtapa;
    ultimoTexto = '';
    if (s.stage === 'generating' && s.progress_max > 1 && s.progress_value != null) {
      const pct = Math.min(100, (100 * s.progress_value) / s.progress_max);
      $('img-progresso').classList.remove('indeterminada');
      $('img-barra').style.width = `${pct}%`;
      ultimoTexto = `passo ${s.progress_value} de ${s.progress_max}`;
    } else if (s.stage === 'generating' && s.message) {
      // Antes do primeiro passo o gerador ainda carrega o RealVisXL; a
      // mensagem do backend diz isso, em vez de um "Gerando" parado.
      ultimaEtapa = s.message.replace(/\.\.\.$/, '…');
      $('img-progresso').classList.add('indeterminada');
    } else {
      $('img-progresso').classList.add('indeterminada');
    }
    atualizarEstado();
  });

  for (const b of document.querySelectorAll('#img-formato button')) {
    b.addEventListener('click', () => escolherFormato(b.dataset.valor));
  }
  $('img-sortear').addEventListener('click', () => {
    $('img-seed').value = String(crypto.getRandomValues(new Uint32Array(1))[0]);
  });
  $('img-prompt').addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && e.ctrlKey) gerar();
  });
  $('img-gerar').addEventListener('click', gerar);
  $('img-gerar').dataset.dica = 'Gerar · Ctrl+Enter';
  $('img-cancelar').addEventListener('click', () => {
    invoke('cancelar_imagem');
    $('img-cancelar').disabled = true;
    ultimoTexto = 'cancelamento solicitado';
    atualizarEstado();
  });
  $('img-reutilizar').addEventListener('click', reutilizar);
  $('img-abrir').addEventListener('click', () => exibida && invoke('abrir_no_windows', { caminho: exibida.file }));
  $('img-pasta').addEventListener('click', () => invoke('abrir_no_windows', { caminho: 'imagens' }));
  window.addEventListener('aba', (e) => {
    if (e.detail === 'imagens') {
      $('img-subtitulo').textContent = `${window.ordis.nomes().imagem}, gerado localmente. O prompt vai exatamente como escrito.`;
      if (!gerando && !/^Concluída|cancelada/.test($('img-estado').textContent)) $('img-estado').textContent = aviso();
      galeria();
    }
  });
})();
