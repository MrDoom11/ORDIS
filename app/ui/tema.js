// Carregado no <head> para aplicar o tema antes de desenhar a página.
// "sistema" deixa a janela e a WebView seguirem o Windows; "claro" e
// "escuro" fixam as duas (a barra de título e o acrílico acompanham).
(function () {
  const CHAVE = 'ordis.tema';
  const ler = () => {
    try {
      return localStorage.getItem(CHAVE) || 'sistema';
    } catch {
      return 'sistema';
    }
  };

  function aplicar(tema) {
    const raiz = document.documentElement;
    if (tema === 'sistema') raiz.removeAttribute('data-tema');
    else raiz.dataset.tema = tema;
    const janela = window.__TAURI__ && window.__TAURI__.window;
    if (janela) {
      const valor = tema === 'claro' ? 'light' : tema === 'escuro' ? 'dark' : null;
      janela.getCurrentWindow().setTheme(valor).catch(() => {});
    }
    window.dispatchEvent(new CustomEvent('tema', { detail: tema }));
  }

  window.ordisTema = {
    atual: ler,
    definir(tema) {
      try {
        localStorage.setItem(CHAVE, tema);
      } catch {}
      aplicar(tema);
    },
  };
  aplicar(ler());
})();
