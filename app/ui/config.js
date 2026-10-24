(function () {
  const { invoke } = window.__TAURI__.core;
  const $ = (id) => document.getElementById(id);
  const { avisar } = window.ordis;

  // ---------- Aparência ----------

  function marcarTema(tema) {
    for (const b of document.querySelectorAll('#tema-opcoes button')) b.classList.toggle('ativo', b.dataset.valor === tema);
  }
  for (const b of document.querySelectorAll('#tema-opcoes button')) {
    b.addEventListener('click', () => window.ordisTema.definir(b.dataset.valor));
  }
  window.addEventListener('tema', (e) => marcarTema(e.detail));
  marcarTema(window.ordisTema.atual());

  // ---------- Proxmox ----------

  function resumoNos(v) {
    const nos = (v.data && v.data.items) || [];
    const nomes = nos.map((n) => `${n.node} (${n.status})`).join(', ');
    return `Conexão validada: ${nos.length} nó(s)${nomes ? ` — ${nomes}` : ''}.`;
  }

  async function carregar() {
    const e = await invoke('proxmox_estado');
    $('px-url').value = e.url;
    $('px-token').value = e.token_id;
    $('px-situacao').textContent = e.configurado ? (e.ca ? 'Configurado' : 'CA não encontrada') : 'Não configurado';
    $('px-situacao').className = `selo ${e.configurado && e.ca ? 'ok' : ''}`;
    $('px-testar').disabled = !e.configurado;
  }

  function lerCa() {
    const f = $('px-ca').files[0];
    return f ? f.text() : Promise.resolve(null);
  }

  async function ocupado(acao) {
    const botoes = [$('px-salvar'), $('px-testar')];
    botoes.forEach((b) => (b.disabled = true));
    $('px-situacao').textContent = 'Testando…';
    $('px-situacao').className = 'selo';
    try {
      avisar(resumoNos(await acao()));
    } catch (e) {
      avisar(String(e), true);
    } finally {
      botoes.forEach((b) => (b.disabled = false));
      await carregar();
    }
  }

  $('px-ca-botao').addEventListener('click', () => $('px-ca').click());
  $('px-ca').addEventListener('change', () => {
    const f = $('px-ca').files[0];
    $('px-ca-nome').textContent = f ? f.name : 'Mantém o certificado atual';
  });
  $('px-salvar').addEventListener('click', () =>
    ocupado(async () => {
      const r = await invoke('proxmox_salvar', {
        url: $('px-url').value,
        tokenId: $('px-token').value,
        segredo: $('px-segredo').value,
        caPem: await lerCa(),
      });
      $('px-segredo').value = '';
      $('px-ca').value = '';
      $('px-ca-nome').textContent = 'Mantém o certificado atual';
      return r;
    }),
  );
  $('px-testar').addEventListener('click', () => ocupado(() => invoke('proxmox_testar')));
  window.addEventListener('aba', (e) => {
    if (e.detail === 'config') carregar();
  });
})();
