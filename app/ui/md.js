// Markdown mínimo para as respostas do agente: blocos de código, títulos,
// listas, citações, tabelas, ênfase e links. Todo texto é escapado antes.
(function () {
  const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

  function inline(s) {
    const codigos = [];
    s = s.replace(/`([^`]+)`/g, (_, c) => `\u0000${codigos.push(c) - 1}\u0000`);
    s = esc(s)
      .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
      .replace(/(^|[^*\w])\*([^*\s][^*]*)\*/g, '$1<em>$2</em>')
      .replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2">$1</a>')
      .replace(/(^|[\s(])(https?:\/\/[^\s<)]+[^\s<).,;:!?])/g, '$1<a href="$2">$2</a>');
    return s.replace(/\u0000(\d+)\u0000/g, (_, i) => `<code>${esc(codigos[i])}</code>`);
  }

  function tabela(linhas) {
    const celulas = (l) => l.trim().replace(/^\||\|$/g, '').split('|').map((c) => inline(c.trim()));
    const cab = celulas(linhas[0]);
    const corpo = linhas.slice(2).map(celulas);
    return `<table><thead><tr>${cab.map((c) => `<th>${c}</th>`).join('')}</tr></thead><tbody>${corpo
      .map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join('')}</tr>`)
      .join('')}</tbody></table>`;
  }

  window.markdown = function (texto) {
    const linhas = texto.replace(/\r/g, '').split('\n');
    const out = [];
    let i = 0;
    while (i < linhas.length) {
      const l = linhas[i];
      const cerca = l.match(/^\s*```(\S*)/);
      if (cerca) {
        const bloco = [];
        i++;
        while (i < linhas.length && !/^\s*```/.test(linhas[i])) bloco.push(linhas[i++]);
        i++;
        out.push(`<pre><code>${esc(bloco.join('\n'))}</code></pre>`);
        continue;
      }
      const titulo = l.match(/^(#{1,4})\s+(.*)/);
      if (titulo) {
        const n = Math.min(titulo[1].length + 2, 6);
        out.push(`<h${n}>${inline(titulo[2])}</h${n}>`);
        i++;
        continue;
      }
      if (/^\s*([-*_])\s*\1\s*\1[\s\-*_]*$/.test(l)) {
        out.push('<hr>');
        i++;
        continue;
      }
      if (/^\s*\|.*\|\s*$/.test(l) && i + 1 < linhas.length && /^\s*\|?[\s:-]+\|[\s|:-]*$/.test(linhas[i + 1])) {
        const bloco = [];
        while (i < linhas.length && /^\s*\|.*\|\s*$/.test(linhas[i])) bloco.push(linhas[i++]);
        out.push(tabela(bloco));
        continue;
      }
      if (/^\s*>/.test(l)) {
        const bloco = [];
        while (i < linhas.length && /^\s*>/.test(linhas[i])) bloco.push(linhas[i++].replace(/^\s*>\s?/, ''));
        out.push(`<blockquote>${window.markdown(bloco.join('\n'))}</blockquote>`);
        continue;
      }
      const item = l.match(/^(\s*)([-*+]|\d+[.)])\s+(.*)/);
      if (item) {
        const ordenada = /\d/.test(item[2]);
        const itens = [];
        while (i < linhas.length) {
          const m = linhas[i].match(/^(\s*)([-*+]|\d+[.)])\s+(.*)/);
          if (m) itens.push(inline(m[3]));
          else if (/^\s{2,}\S/.test(linhas[i]) && itens.length) itens[itens.length - 1] += '<br>' + inline(linhas[i].trim());
          else break;
          i++;
        }
        const tag = ordenada ? 'ol' : 'ul';
        out.push(`<${tag}>${itens.map((t) => `<li>${t}</li>`).join('')}</${tag}>`);
        continue;
      }
      if (!l.trim()) {
        i++;
        continue;
      }
      const par = [];
      while (i < linhas.length && linhas[i].trim() && !/^\s*(```|#{1,4}\s|>|\||([-*+]|\d+[.)])\s)/.test(linhas[i])) {
        par.push(inline(linhas[i++]));
      }
      if (!par.length) par.push(inline(linhas[i++]));
      out.push(`<p>${par.join('<br>')}</p>`);
    }
    return out.join('');
  };
})();
