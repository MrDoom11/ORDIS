(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const { el, icone, botaoIcone, avisar, copiar } = window.ordis;

  const ROTULOS = {
    read: 'Lendo arquivo',
    session_status: 'Consultando a sessão',
    web_fetch: 'Lendo página',
    ordis_buscar_web: 'Pesquisando na web',
    proxmox_consultar: 'Consultando o Proxmox',
    ordis_gerar_imagem: 'Gerando imagem',
  };
  const SUGESTOES = [
    'Resuma o estado do Proxmox',
    'Pesquise as notícias de tecnologia de hoje',
    'Gere uma imagem de um farol ao entardecer',
  ];
  const CHAVE_LATERAL = 'ordis.conversas-ocultas';

  let atual = null; // id da sessão aberta
  let respondendo = false;
  let anexos = []; // data URLs
  let lista = []; // resumos das sessões
  let bloco = null; // resposta do assistente em construção
  let segmento = null; // { raciocinio, corpo, texto }
  let mensagemEstado = null; // { texto, erro } enquanto houver algo a dizer
  let qwen = null; // situação da inferência no último painel
  let painel = null;
  let tokensAoVivo = null;
  const cacheImagens = new Map();

  // ---------- Renderização ----------

  function imagem(caminho, destino) {
    const img = el('img');
    img.alt = caminho.split(/[\\/]/).pop();
    destino.append(img);
    const pronta = cacheImagens.get(caminho);
    if (pronta) img.src = pronta;
    else
      invoke('imagem', { caminho })
        .then((url) => {
          cacheImagens.set(caminho, url);
          img.src = url;
        })
        .catch(() => (img.alt = 'Imagem indisponível'));
  }

  function grupoImagens(caminhos, destino) {
    if (!caminhos.length) return;
    const g = el('div', 'imgs');
    caminhos.forEach((c) => imagem(c, g));
    destino.append(g);
  }

  // Linhas "MEDIA:caminho" (convenção antiga) viram imagens.
  function separarMidia(texto) {
    const midias = [];
    const limpo = texto.replace(/^\s*MEDIA:\s*(.+?)\s*$/gm, (_, c) => {
      midias.push(c);
      return '';
    });
    return { limpo, midias };
  }

  function preencherCorpo(corpo, texto) {
    const { limpo, midias } = separarMidia(texto);
    corpo.innerHTML = window.markdown(limpo);
    for (const pre of corpo.querySelectorAll('pre')) {
      const b = botaoIcone('i-copiar', 'Copiar código', 'copiar-codigo');
      b.addEventListener('click', () => copiado(b, pre.querySelector('code').textContent));
      pre.append(b);
    }
    grupoImagens(midias, corpo);
  }

  function copiado(botao, texto) {
    copiar(texto);
    botao.replaceChildren(icone('i-ok'));
    setTimeout(() => botao.replaceChildren(icone('i-copiar')), 1400);
  }

  function blocoUsuario(texto, imagens) {
    const div = el('div', 'msg user', texto);
    grupoImagens(imagens || [], div);
    return div;
  }

  function novoBloco() {
    const div = el('div', 'msg assistant');
    div.textos = [];
    $('mensagens').append(div);
    return div;
  }

  // Barra de ações sob a resposta: copiar sempre, refazer só na última.
  function fecharBloco(b, ultima) {
    b.querySelector('.acoes-msg')?.remove();
    const barra = el('div', 'acoes-msg');
    const texto = b.textos.map((t) => separarMidia(t).limpo.trim()).filter(Boolean).join('\n\n');
    if (texto) {
      const c = botaoIcone('i-copiar', 'Copiar resposta');
      c.addEventListener('click', () => copiado(c, texto));
      barra.append(c);
    }
    if (ultima) {
      for (const outro of document.querySelectorAll('.msg.assistant.ultima')) {
        outro.classList.remove('ultima');
        outro.querySelector('.refazer')?.remove();
      }
      b.classList.add('ultima');
      const r = botaoIcone('i-refazer', 'Refazer resposta', 'refazer');
      r.addEventListener('click', refazer);
      barra.append(r);
    }
    if (barra.children.length) b.append(barra);
  }

  function detalhesRaciocinio(texto, aberto) {
    const d = el('details', 'raciocinio');
    d.open = aberto;
    d.append(el('summary', '', 'Raciocínio'), el('div', 'conteudo', texto));
    return d;
  }

  function detalhesFerramenta(ch) {
    const d = el('details', 'ferramenta rodando');
    d.dataset.id = ch.id;
    d.dataset.nome = ch.nome;
    let args = ch.argumentos;
    try {
      args = JSON.stringify(JSON.parse(args), null, 2);
    } catch {}
    d.append(el('summary', '', ROTULOS[ch.nome] || ch.nome), el('div', 'conteudo', `${ch.nome} ${args}`));
    return d;
  }

  function preencherResultado(blocoAlvo, id, texto, erro, imagens) {
    const d = blocoAlvo.querySelector(`details.ferramenta[data-id="${CSS.escape(id)}"]`);
    if (!d) return;
    const nome = d.dataset.nome;
    d.classList.remove('rodando');
    d.classList.toggle('erro', !!erro);
    d.querySelector('summary').textContent = `${ROTULOS[nome] || nome}${erro ? ' — falhou' : ''}`;
    let corpo = texto;
    try {
      corpo = JSON.stringify(JSON.parse(texto), null, 2);
    } catch {}
    d.querySelector('.conteudo').textContent += `\n\n→ ${corpo}`;
    // Imagens geradas aparecem fora do detalhe, direto na conversa.
    if (imagens && imagens.length && nome !== 'read') grupoImagens(imagens, blocoAlvo);
    else if (imagens && imagens.length) grupoImagens(imagens, d.querySelector('.conteudo'));
  }

  function renderizar(sessao) {
    const area = $('mensagens');
    area.replaceChildren();
    bloco = null;
    if (!sessao.mensagens.length) {
      area.append(vazio());
      return;
    }
    const blocos = [];
    for (const m of sessao.mensagens) {
      if (m.papel === 'user') {
        area.append(blocoUsuario(m.texto, m.imagens));
        bloco = null;
      } else if (m.papel === 'assistant') {
        if (!bloco) blocos.push((bloco = novoBloco()));
        if (m.raciocinio) bloco.append(detalhesRaciocinio(m.raciocinio, false));
        if (m.texto.trim()) {
          const corpo = el('div', 'corpo');
          preencherCorpo(corpo, m.texto);
          bloco.append(corpo);
          bloco.textos.push(m.texto);
        }
        (m.chamadas || []).forEach((ch) => bloco.append(detalhesFerramenta(ch)));
      } else if (m.papel === 'tool' && bloco) {
        preencherResultado(bloco, m.id_chamada, m.texto, m.erro, m.imagens || []);
      }
    }
    const ultimaEhResposta = sessao.mensagens[sessao.mensagens.length - 1].papel !== 'user';
    blocos.forEach((b, i) => fecharBloco(b, ultimaEhResposta && i === blocos.length - 1));
    bloco = null;
    rolar(true);
  }

  function vazio() {
    const v = el('div', 'vazio');
    const img = el('img');
    img.src = 'ordis.png';
    img.alt = '';
    const sugestoes = el('div', 'sugestoes');
    for (const s of SUGESTOES) {
      const b = el('button', '', s);
      b.addEventListener('click', () => {
        $('entrada').value = s;
        ajustarAltura();
        $('entrada').focus();
      });
      sugestoes.append(b);
    }
    v.append(img, el('h2', '', 'Como posso ajudar, operador?'), sugestoes);
    return v;
  }

  function pertoDoFim() {
    const area = $('mensagens');
    return area.scrollHeight - area.scrollTop - area.clientHeight < 120;
  }

  function rolar(forcar) {
    const area = $('mensagens');
    if (forcar || pertoDoFim()) area.scrollTop = area.scrollHeight;
  }

  // ---------- Conversas ----------

  const normalizar = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();

  function grupoDe(segundos) {
    const hoje = new Date();
    hoje.setHours(0, 0, 0, 0);
    const dias = (hoje.getTime() / 1000 - segundos) / 86400;
    if (dias <= 0) return 'Hoje';
    if (dias <= 1) return 'Ontem';
    if (dias <= 7) return 'Últimos 7 dias';
    if (dias <= 30) return 'Últimos 30 dias';
    return 'Mais antigas';
  }

  function desenharLista() {
    const filtro = normalizar($('busca-conversas').value.trim());
    const visiveis = filtro ? lista.filter((s) => normalizar(s.titulo).includes(filtro)) : lista;
    const destino = $('lista-conversas');
    destino.replaceChildren();
    if (!visiveis.length) {
      destino.append(el('p', 'lista-vazia', filtro ? 'Nenhuma conversa encontrada.' : 'Suas conversas aparecem aqui.'));
      return;
    }
    let grupo = null;
    for (const s of visiveis) {
      const g = grupoDe(s.atualizada);
      if (g !== grupo) destino.append(el('div', 'grupo', (grupo = g)));
      destino.append(itemConversa(s));
    }
  }

  function itemConversa(s) {
    const item = el('div', `item-conversa${s.id === atual ? ' ativa' : ''}`);
    const titulo = el('span', 'titulo', s.titulo);
    const editar = botaoIcone('i-lapis', 'Renomear');
    const apagar = botaoIcone('i-lixo', 'Apagar', 'apagar');
    editar.addEventListener('click', (e) => {
      e.stopPropagation();
      renomear(s, titulo);
    });
    apagar.addEventListener('click', (e) => {
      e.stopPropagation();
      confirmarApagar(s, item, titulo, apagar);
    });
    item.append(titulo, editar, apagar);
    item.addEventListener('click', () => abrir(s.id));
    item.addEventListener('dblclick', () => renomear(s, titulo));
    return item;
  }

  function renomear(s, alvo) {
    const campo = el('input');
    campo.value = s.titulo;
    let feito = false;
    const concluir = async (salvar) => {
      if (feito) return;
      feito = true;
      const novo = campo.value.trim();
      if (salvar && novo && novo !== s.titulo) {
        try {
          await invoke('renomear_sessao', { id: s.id, titulo: novo });
          if (s.id === atual) $('titulo-conversa').textContent = novo;
        } catch (e) {
          avisar(String(e), true);
        }
      }
      carregarLista();
    };
    campo.addEventListener('click', (e) => e.stopPropagation());
    campo.addEventListener('dblclick', (e) => e.stopPropagation());
    campo.addEventListener('keydown', (e) => {
      e.stopPropagation();
      if (e.key === 'Enter') concluir(true);
      if (e.key === 'Escape') concluir(false);
    });
    campo.addEventListener('blur', () => concluir(true));
    alvo.replaceWith(campo);
    campo.focus();
    campo.select();
  }

  // Apagar pede um segundo clique em vez de abrir uma caixa de diálogo.
  function confirmarApagar(s, item, titulo, botao) {
    if (respondendo && s.id === atual) return;
    if (!item.classList.contains('confirmando')) {
      item.classList.add('confirmando');
      titulo.textContent = 'Clique de novo para apagar';
      titulo.style.color = 'var(--erro)';
      botao.style.display = 'inline-flex';
      setTimeout(() => {
        if (!item.isConnected) return;
        item.classList.remove('confirmando');
        titulo.textContent = s.titulo;
        titulo.style.color = '';
        botao.style.display = '';
      }, 3000);
      return;
    }
    invoke('apagar_sessao', { id: s.id })
      .then(() => {
        if (s.id === atual) novaConversa();
        else carregarLista();
        avisar('Conversa apagada.');
      })
      .catch((e) => avisar(String(e), true));
  }

  async function carregarLista() {
    lista = await invoke('sessoes');
    desenharLista();
  }

  async function abrir(id) {
    if (respondendo) return;
    try {
      const s = await invoke('sessao', { id });
      atual = s.id;
      $('titulo-conversa').textContent = s.titulo;
      renderizar(s);
      desenharLista();
      $('entrada').focus();
    } catch (e) {
      avisar(String(e), true);
    }
  }

  function novaConversa() {
    if (respondendo) return;
    atual = null;
    $('titulo-conversa').textContent = 'Nova conversa';
    $('mensagens').replaceChildren(vazio());
    mensagemEstado = null;
    desenharEstado();
    carregarLista();
    $('entrada').focus();
  }

  function alternarLateral(ocultar) {
    const vista = $('vista-agente');
    ocultar = ocultar ?? !vista.classList.contains('sem-conversas');
    vista.classList.toggle('sem-conversas', ocultar);
    try {
      localStorage.setItem(CHAVE_LATERAL, ocultar ? '1' : '');
    } catch {}
  }

  // ---------- Linha de estado ----------

  function estado(texto, erro) {
    mensagemEstado = texto ? { texto, erro: !!erro } : null;
    desenharEstado();
  }

  function desenharEstado() {
    const alvo = $('estado-agente');
    alvo.classList.toggle('erro', !!(mensagemEstado && mensagemEstado.erro));
    if (mensagemEstado) {
      const extra = respondendo && tokensAoVivo ? ` · ${tokensAoVivo}` : '';
      alvo.replaceChildren(mensagemEstado.texto + extra);
      alvo.title = mensagemEstado.texto;
      return;
    }
    alvo.title = '';
    const troca = qwen !== 'pronto' && window.ordis.emTroca(painel);
    const ponto = el('span', `ponto ${qwen === 'pronto' ? 'pronto' : qwen === 'iniciando' || troca ? 'iniciando' : ''}`);
    const n = window.ordis.nomes().conversa;
    const texto =
      qwen === 'pronto'
        ? `${n} pronto`
        : troca
          ? qwen === 'iniciando'
            ? `${n} voltando para a GPU…`
            : `${n} fora da GPU enquanto a imagem é gerada · volta sozinho`
          : qwen === 'iniciando'
            ? `${n} iniciando…`
            : qwen
              ? `${n} desligado · carrega ao enviar (~30 s)`
              : '';
    alvo.replaceChildren(...(texto ? [ponto, texto] : []));
  }

  // ---------- Envio ----------

  function definirRespondendo(sim) {
    respondendo = sim;
    const b = $('enviar');
    b.classList.toggle('parar', sim);
    b.replaceChildren(icone(sim ? 'i-parar' : 'i-enviar'));
    b.dataset.dica = sim ? 'Parar · Esc' : 'Enviar · Enter';
    $('nova').disabled = sim;
    atualizarBotaoEnviar();
  }

  function atualizarBotaoEnviar() {
    $('enviar').disabled = !respondendo && !$('entrada').value.trim() && !anexos.length;
  }

  function interromper() {
    if (!respondendo) return;
    invoke('interromper');
    estado('Interrompendo…');
  }

  async function responder(chamada) {
    bloco = novoBloco();
    segmento = null;
    rolar(true);
    definirRespondendo(true);
    estado('Pensando…');
    let falhou = false;
    try {
      await chamada();
      estado('');
    } catch (e) {
      falhou = true;
      estado(String(e), true);
    } finally {
      tokensAoVivo = null;
      definirRespondendo(false);
      finalizarSegmento();
      if (bloco) {
        if (!bloco.children.length && !falhou) bloco.remove();
        else fecharBloco(bloco, true);
      }
      bloco = null;
      carregarLista();
    }
  }

  async function enviar() {
    if (respondendo) return interromper();
    const texto = $('entrada').value.trim();
    if (!texto && !anexos.length) return;
    try {
      if (!atual) {
        atual = (await invoke('nova_sessao')).id;
        $('titulo-conversa').textContent = (texto || 'Imagem').slice(0, 60);
      }
    } catch (e) {
      estado(String(e), true);
      return;
    }
    const enviados = anexos;
    anexos = [];
    mostrarAnexos();
    $('entrada').value = '';
    ajustarAltura();

    const area = $('mensagens');
    area.querySelector('.vazio')?.remove();
    const bolha = el('div', 'msg user', texto);
    if (enviados.length) {
      const g = el('div', 'imgs');
      enviados.forEach((url) => {
        const img = el('img');
        img.src = url;
        g.append(img);
      });
      bolha.append(g);
    }
    area.append(bolha);
    await responder(() => invoke('enviar', { id: atual, texto, anexos: enviados }));
  }

  async function refazer() {
    if (respondendo || !atual) return;
    const ultimo = $('mensagens').lastElementChild;
    if (ultimo && ultimo.classList.contains('assistant')) ultimo.remove();
    await responder(() => invoke('refazer', { id: atual }));
  }

  function finalizarSegmento() {
    if (!segmento) return;
    // Desenha o que chegou depois do último quadro agendado.
    preencherCorpo(segmento.corpo, segmento.texto);
    segmento.corpo.classList.remove('cursor');
    if (segmento.raciocinio) {
      segmento.raciocinio.open = false;
      segmento.raciocinio.classList.remove('rodando');
    }
    if (segmento.texto.trim()) bloco?.textos.push(segmento.texto);
    else segmento.corpo.remove();
    segmento = null;
  }

  let quadroPendente = false;
  function desenharTexto() {
    if (quadroPendente) return;
    quadroPendente = true;
    requestAnimationFrame(() => {
      quadroPendente = false;
      if (!segmento) return;
      preencherCorpo(segmento.corpo, segmento.texto);
      rolar(false);
    });
  }

  listen('agente', ({ payload }) => {
    if (payload.sessao !== atual || !bloco) return;
    const d = payload.dados;
    switch (payload.tipo) {
      case 'inicio':
        finalizarSegmento();
        segmento = { raciocinio: null, corpo: el('div', 'corpo cursor'), texto: '' };
        bloco.append(segmento.corpo);
        break;
      case 'raciocinio':
        if (!segmento) break;
        if (!segmento.raciocinio) {
          segmento.raciocinio = detalhesRaciocinio('', false);
          segmento.raciocinio.classList.add('rodando');
          bloco.insertBefore(segmento.raciocinio, segmento.corpo);
          estado('Raciocinando…');
        }
        segmento.raciocinio.querySelector('.conteudo').textContent += d.texto;
        if (segmento.raciocinio.open) rolar(false);
        break;
      case 'texto':
        if (!segmento) break;
        if (segmento.raciocinio) {
          segmento.raciocinio.classList.remove('rodando');
          segmento.raciocinio.open = false;
        }
        segmento.texto += d.texto;
        if (!mensagemEstado || mensagemEstado.texto !== 'Respondendo…') estado('Respondendo…');
        desenharTexto();
        break;
      case 'ferramenta':
        if (d.nome === 'ordis_gerar_imagem') window.ordis.definirTroca(true);
        finalizarSegmento();
        bloco.append(detalhesFerramenta(d));
        estado(`${ROTULOS[d.nome] || d.nome}…`);
        rolar(false);
        break;
      case 'resultado':
        if (bloco.querySelector(`details.ferramenta[data-id="${CSS.escape(d.id)}"]`)?.dataset.nome === 'ordis_gerar_imagem')
          window.ordis.definirTroca(false);
        preencherResultado(bloco, d.id, d.texto, d.erro, d.imagens);
        rolar(false);
        break;
      case 'status':
        estado(d.texto);
        break;
    }
  });

  window.addEventListener('painel', ({ detail }) => {
    painel = detail;
    qwen = detail.servicos.find((s) => s.nome === 'inferencia').situacao;
    const t = detail.tokens;
    tokensAoVivo =
      respondendo && segmento && t && typeof t.tokens_per_second === 'number'
        ? `${t.tokens_per_second.toFixed(1)} tokens/s`
        : null;
    desenharEstado();
  });

  // ---------- Anexos ----------

  function mostrarAnexos() {
    $('anexos').replaceChildren(
      ...anexos.map((url, i) => {
        const div = el('div', 'anexo');
        const img = el('img');
        img.src = url;
        const tirar = el('button');
        tirar.append(icone('i-x'));
        tirar.dataset.dica = 'Remover';
        tirar.addEventListener('click', () => {
          anexos.splice(i, 1);
          mostrarAnexos();
        });
        div.append(img, tirar);
        return div;
      }),
    );
    atualizarBotaoEnviar();
  }

  function lerArquivos(arquivos) {
    for (const f of arquivos) {
      if (!f.type.startsWith('image/')) continue;
      if (f.size > 15 * 1024 * 1024) {
        avisar(`${f.name} passa de 15 MB.`, true);
        continue;
      }
      const leitor = new FileReader();
      leitor.onload = () => {
        anexos.push(leitor.result);
        mostrarAnexos();
      };
      leitor.readAsDataURL(f);
    }
  }

  // ---------- Eventos ----------

  function ajustarAltura() {
    const t = $('entrada');
    t.style.height = 'auto';
    t.style.height = `${Math.min(t.scrollHeight, 220)}px`;
    t.style.overflowY = t.scrollHeight > 220 ? 'auto' : 'hidden';
    atualizarBotaoEnviar();
  }

  $('entrada').addEventListener('input', ajustarAltura);
  $('entrada').addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      if (!respondendo) enviar();
    }
  });
  $('entrada').addEventListener('paste', (e) => {
    const imgs = [...e.clipboardData.files].filter((f) => f.type.startsWith('image/'));
    if (imgs.length) {
      e.preventDefault();
      lerArquivos(imgs);
    }
  });
  $('enviar').addEventListener('click', enviar);
  $('nova').addEventListener('click', novaConversa);
  $('anexar').addEventListener('click', () => $('arquivo').click());
  $('arquivo').addEventListener('change', (e) => {
    lerArquivos(e.target.files);
    e.target.value = '';
  });
  $('busca-conversas').addEventListener('input', desenharLista);
  $('busca-conversas').addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.target.value = '';
      desenharLista();
      $('entrada').focus();
    }
  });
  $('alternar-conversas').addEventListener('click', () => alternarLateral());
  $('titulo-conversa').addEventListener('dblclick', () => {
    const s = lista.find((x) => x.id === atual);
    if (s) {
      alternarLateral(false);
      const alvo = [...document.querySelectorAll('.item-conversa.ativa .titulo')][0];
      if (alvo) renomear(s, alvo);
    }
  });

  const chat = document.querySelector('.chat');
  chat.addEventListener('dragover', (e) => {
    if (![...e.dataTransfer.types].includes('Files')) return;
    e.preventDefault();
    $('compositor').classList.add('arrastando');
  });
  chat.addEventListener('dragleave', (e) => {
    if (!chat.contains(e.relatedTarget)) $('compositor').classList.remove('arrastando');
  });
  chat.addEventListener('drop', (e) => {
    e.preventDefault();
    $('compositor').classList.remove('arrastando');
    lerArquivos(e.dataTransfer.files);
  });

  $('mensagens').addEventListener('scroll', () => {
    $('ir-fim').hidden = pertoDoFim();
  });
  $('ir-fim').addEventListener('click', () => {
    const area = $('mensagens');
    area.scrollTo({ top: area.scrollHeight, behavior: 'smooth' });
  });

  $('mensagens').addEventListener('click', (e) => {
    const link = e.target.closest('a[href]');
    if (link) {
      e.preventDefault();
      invoke('abrir_url', { url: link.href });
      return;
    }
    if (e.target.tagName === 'IMG' && e.target.src && !e.target.closest('.vazio')) {
      const z = el('div', 'zoom');
      const img = el('img');
      img.src = e.target.src;
      z.append(img);
      z.addEventListener('click', () => z.remove());
      document.body.append(z);
    }
  });

  document.addEventListener('keydown', (e) => {
    const naAba = !$('vista-agente').hidden;
    if (e.key === 'Escape') {
      const z = document.querySelector('.zoom');
      if (z) return z.remove();
      if (naAba && respondendo) interromper();
      return;
    }
    if (!e.ctrlKey || e.shiftKey || e.altKey) return;
    const tecla = e.key.toLowerCase();
    if (tecla === 'n') {
      e.preventDefault();
      window.ordis.mostrarAba('agente');
      novaConversa();
    } else if (tecla === 'b' && naAba) {
      e.preventDefault();
      alternarLateral();
    } else if (tecla === 'k') {
      e.preventDefault();
      window.ordis.mostrarAba('agente');
      alternarLateral(false);
      $('busca-conversas').focus();
      $('busca-conversas').select();
    }
  });
  window.addEventListener('troca', desenharEstado);
  window.addEventListener('aba', (e) => {
    if (e.detail === 'agente') setTimeout(() => $('entrada').focus(), 0);
  });

  try {
    if (localStorage.getItem(CHAVE_LATERAL)) alternarLateral(true);
  } catch {}
  $('mensagens').append(vazio());
  definirRespondendo(false);
  carregarLista();
  $('entrada').focus();
})();
