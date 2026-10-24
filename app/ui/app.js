const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const SERVICOS = {
  inferencia: { titulo: 'Inferência', detalhe: () => `${nomes.conversa} · TabbyAPI` },
  imagens: { titulo: 'Imagens', detalhe: () => `${nomes.imagem} · ComfyUI` },
};
const SITUACAO = { pronto: 'Pronto', iniciando: 'Iniciando…', desligado: 'Desligado' };
const TEMAS = {
  sistema: { icone: 'i-sistema', nome: 'Sistema' },
  claro: { icone: 'i-sol', nome: 'Claro' },
  escuro: { icone: 'i-lua', nome: 'Escuro' },
};

const $ = (id) => document.getElementById(id);
let ocupado = false;
let ultimoPainel = null;
let trocando = false; // geração de imagem em curso neste app: o modelo de conversa sai da GPU e volta
// Nomes curtos dos modelos configurados (Configurações > Modelos), vindos do painel.
let nomes = { conversa: 'Qwen3.8-27B', imagem: 'RealVisXL_V5.0' };

// ---------- Utilidades compartilhadas pelas abas ----------

window.ordis = {
  el(tag, classe, texto) {
    const e = document.createElement(tag);
    if (classe) e.className = classe;
    if (texto !== undefined) e.textContent = texto;
    return e;
  },
  icone(nome) {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
    use.setAttribute('href', `#${nome}`);
    svg.append(use);
    return svg;
  },
  botaoIcone(nome, dica, classe = '') {
    const b = window.ordis.el('button', `icone ${classe}`.trim());
    b.dataset.dica = dica;
    b.append(window.ordis.icone(nome));
    return b;
  },
  avisar(texto, erro = false) {
    const a = window.ordis.el('div', `aviso${erro ? ' erro' : ''}`, texto);
    $('avisos').append(a);
    setTimeout(() => {
      a.classList.add('saindo');
      setTimeout(() => a.remove(), 220);
    }, erro ? 6000 : 3200);
  },
  async copiar(texto) {
    try {
      await navigator.clipboard.writeText(texto);
    } catch {
      const t = document.createElement('textarea');
      t.value = texto;
      document.body.append(t);
      t.select();
      document.execCommand('copy');
      t.remove();
    }
  },
  mostrarAba,
  nomes: () => nomes,
  // Há troca de modelo quando este app gera uma imagem ou quando o ComfyUI
  // está ligado (geração iniciada pelo terminal, por exemplo).
  emTroca(p) {
    return trocando || !!(p && p.servicos.some((s) => s.nome === 'imagens' && s.situacao !== 'desligado'));
  },
  definirTroca(sim) {
    trocando = sim;
    if (ultimoPainel) mostrarGlobal(ultimoPainel);
    if (!sim) atualizar();
    window.dispatchEvent(new CustomEvent('troca', { detail: sim }));
  },
};
const { el, icone } = window.ordis;

// ---------- Abas, tema e dicas ----------

function mostrarAba(vista) {
  for (const a of document.querySelectorAll('.aba[data-vista]')) a.classList.toggle('ativa', a.dataset.vista === vista);
  for (const v of document.querySelectorAll('.vista')) v.hidden = v.id !== `vista-${vista}`;
  window.dispatchEvent(new CustomEvent('aba', { detail: vista }));
}

for (const aba of document.querySelectorAll('.aba[data-vista]')) {
  aba.addEventListener('click', () => mostrarAba(aba.dataset.vista));
}

function iconeTema(tema) {
  const t = TEMAS[tema] || TEMAS.sistema;
  $('tema-rapido').replaceChildren(icone(t.icone));
  $('tema-rapido').dataset.dica = `Tema: ${t.nome}`;
}
window.addEventListener('tema', (e) => iconeTema(e.detail));
iconeTema(window.ordisTema.atual());
$('tema-rapido').addEventListener('click', () => {
  const ordem = Object.keys(TEMAS);
  const proximo = ordem[(ordem.indexOf(window.ordisTema.atual()) + 1) % ordem.length];
  window.ordisTema.definir(proximo);
  mostrarDica($('tema-rapido'));
});

// Dicas próprias no lugar do title, que demora e destoa do tema.
let alvoDica = null;
function mostrarDica(alvo) {
  const d = $('dica');
  alvoDica = alvo;
  d.textContent = alvo.dataset.dica;
  d.hidden = false;
  const r = alvo.getBoundingClientRect();
  const doTrilho = alvo.closest('.trilho');
  let x = doTrilho ? r.right + 8 : r.left + r.width / 2 - d.offsetWidth / 2;
  let y = doTrilho ? r.top + r.height / 2 - d.offsetHeight / 2 : r.top - d.offsetHeight - 6;
  if (!doTrilho && y < 4) y = r.bottom + 6;
  x = Math.max(4, Math.min(x, innerWidth - d.offsetWidth - 4));
  d.style.left = `${x}px`;
  d.style.top = `${y}px`;
}
document.addEventListener('mouseover', (e) => {
  const alvo = e.target.closest('[data-dica]');
  if (alvo === alvoDica) return;
  if (alvo) mostrarDica(alvo);
  else {
    alvoDica = null;
    $('dica').hidden = true;
  }
});
document.addEventListener('mousedown', () => {
  alvoDica = null;
  $('dica').hidden = true;
});

document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && !e.shiftKey && !e.altKey && /^[1-4]$/.test(e.key)) {
    e.preventDefault();
    mostrarAba(['agente', 'imagens', 'painel', 'config'][Number(e.key) - 1]);
  }
  if (e.key === 'Escape') $('popover-servicos').hidden = true;
});

// ---------- Painel e estado global ----------

function registrar(texto, erro = false) {
  const li = el('li', erro ? 'erro' : '');
  const hora = el('time', '', new Date().toLocaleTimeString('pt-BR'));
  li.append(hora, texto);
  $('atividade').prepend(li);
  $('atividade-vazia').hidden = true;
}

function cartaoServico({ nome, porta, situacao }) {
  const info = SERVICOS[nome];
  const div = el('div', `cartao servico ${situacao}`);
  const linha = el('div', 'situacao');
  linha.append(el('span', 'ponto'), SITUACAO[situacao]);
  div.append(el('div', 'nome', info.titulo), el('div', 'detalhe', `${info.detalhe()} · porta ${porta}`), linha);
  return div;
}

function lerVram(texto) {
  // nvidia-smi: "1234 MiB, 16311 MiB"
  const m = texto && texto.match(/(\d+)\s*MiB,\s*(\d+)\s*MiB/);
  return m ? { usado: Number(m[1]) / 1024, total: Number(m[2]) / 1024 } : null;
}

function mostrarVram(v) {
  $('vram-texto').textContent = v ? `${v.usado.toFixed(1)} / ${v.total.toFixed(1)} GB` : 'Indisponível';
  $('vram-barra').style.width = v ? `${Math.min(100, (v.usado / v.total) * 100)}%` : '0';
}

function mostrarTokens(t, inferenciaLigada) {
  const ultima = t && t.last_generation;
  if (inferenciaLigada && t && typeof t.tokens_per_second === 'number') {
    $('tokens').textContent = `${t.tokens_per_second.toFixed(1)} tokens/s`;
    $('tokens-detalhe').textContent = t.status === 'prefill' ? 'Preparando resposta' : 'Ao vivo';
  } else if (ultima && typeof ultima.tokens_per_second === 'number') {
    $('tokens').textContent = `${ultima.tokens_per_second.toFixed(1)} tokens/s`;
    $('tokens-detalhe').textContent = `Última geração · ${ultima.tokens} tokens`;
  } else {
    $('tokens').textContent = '—';
    $('tokens-detalhe').textContent = inferenciaLigada ? 'Sem geração recente' : 'Motor desligado';
  }
}

function algumLigado(p) {
  return p.servicos.some((s) => s.situacao !== 'desligado');
}

function mostrarControles(p) {
  const ligado = p && algumLigado(p);
  const rotulo = ocupado ? 'Aguarde…' : ligado ? 'Desligar' : 'Iniciar';
  for (const b of [$('alternar-servicos'), $('pop-acao')]) {
    b.disabled = ocupado || !p;
    b.classList.toggle('primario', !ligado);
  }
  $('alternar-servicos').querySelector('span').textContent = rotulo;
  $('pop-acao').textContent = rotulo;
}

function mostrarGlobal(p) {
  const inferencia = p.servicos.find((s) => s.nome === 'inferencia');
  const troca = window.ordis.emTroca(p) && inferencia.situacao !== 'pronto';
  const situacao = troca || p.servicos.some((s) => s.situacao === 'iniciando') ? 'iniciando' : inferencia.situacao;
  $('status-global').querySelector('.ponto').className = `ponto ${situacao}`;
  $('status-global').dataset.dica = troca
    ? inferencia.situacao === 'iniciando'
      ? `${nomes.conversa} voltando para a GPU…`
      : `${nomes.conversa} fora da GPU enquanto a imagem é gerada`
    : `${nomes.conversa}: ${SITUACAO[inferencia.situacao].toLowerCase()}`;
  $('pop-linhas').replaceChildren(
    ...p.servicos.map((s) => {
      const l = el('div', 'linha-servico');
      l.append(el('span', `ponto ${s.situacao}`), el('span', '', SERVICOS[s.nome].titulo), el('span', '', SITUACAO[s.situacao]));
      return l;
    }),
  );
  const v = lerVram(p.vram);
  $('pop-vram').textContent = v ? `GPU: ${v.usado.toFixed(1)} de ${v.total.toFixed(1)} GB em uso` : 'GPU: memória indisponível';
}

async function atualizar() {
  try {
    const p = await invoke('painel');
    ultimoPainel = p;
    if (p.modelos) nomes = p.modelos;
    window.dispatchEvent(new CustomEvent('painel', { detail: p }));
    $('servicos').replaceChildren(...p.servicos.map(cartaoServico));
    const ligado = p.servicos.find((s) => s.nome === 'inferencia').situacao === 'pronto';
    mostrarVram(lerVram(p.vram));
    mostrarTokens(p.tokens, ligado);
    mostrarGlobal(p);
    if (!ocupado) {
      $('resumo').textContent = ligado
        ? 'ORDIS pronto. O modelo está carregado na GPU.'
        : algumLigado(p)
          ? 'Serviços parcialmente ligados.'
          : 'ORDIS desligado. A GPU está livre.';
    }
    mostrarControles(p);
  } catch (e) {
    registrar(`Falha ao consultar o estado: ${e}`, true);
  }
}

async function alternarServicos() {
  if (ocupado || !ultimoPainel) return;
  const desligar = algumLigado(ultimoPainel);
  const rotulo = desligar ? 'Desligando o ORDIS…' : 'Iniciando o ORDIS…';
  ocupado = true;
  mostrarControles(ultimoPainel);
  $('resumo').textContent = rotulo;
  registrar(rotulo);
  try {
    await invoke(desligar ? 'parar' : 'iniciar');
    registrar('Concluído.');
    window.ordis.avisar(desligar ? 'ORDIS desligado. GPU livre.' : 'ORDIS pronto.');
  } catch (e) {
    registrar(String(e), true);
    window.ordis.avisar(String(e), true);
  } finally {
    ocupado = false;
    await atualizar();
  }
}

listen('progresso', (ev) => {
  $('resumo').textContent = ev.payload;
  registrar(ev.payload);
});

$('alternar-servicos').addEventListener('click', alternarServicos);
$('pop-acao').addEventListener('click', alternarServicos);
$('logs').addEventListener('click', () => invoke('abrir_logs'));
$('status-global').addEventListener('click', (e) => {
  e.stopPropagation();
  $('popover-servicos').hidden = !$('popover-servicos').hidden;
});
document.addEventListener('click', (e) => {
  if (!e.target.closest('#popover-servicos')) $('popover-servicos').hidden = true;
});

atualizar();
setInterval(() => {
  if (!ocupado) atualizar();
}, 3000);

// Abertura: a janela nasce oculta. Se ainda não apareceu (menos de 500 ms),
// some com o anel e mostra a interface pronta; se o Rust já a mostrou com o
// anel girando, o anel sai com um fade.
window.addEventListener('load', async () => {
  const janela = window.__TAURI__.window.getCurrentWindow();
  const visivel = await janela.isVisible().catch(() => true);
  const anel = $('abertura');
  document.documentElement.classList.remove('abrindo');
  if (visivel) {
    anel.classList.add('saindo');
    anel.addEventListener('transitionend', () => anel.remove(), { once: true });
  } else {
    anel.remove();
    janela.show();
  }
});
