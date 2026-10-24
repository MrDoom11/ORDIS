#!/bin/sh
# Procura dados pessoais e segredos no texto recebido pela entrada padrão
# (linhas adicionadas de um diff). Mostra as linhas suspeitas e sai com 1
# se achar algo.
#
# Os padrões genéricos ficam aqui. Os pessoais (nome, e-mail, IP, nome do
# servidor) ficam em local/padroes-privados.txt, fora do Git: um por linha,
# comparados sem diferenciar maiúsculas; linhas começando com # são ignoradas.

raiz=$(git rev-parse --show-toplevel)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cat > "$tmp/texto"

cat > "$tmp/genericos" <<'EOF'
(^|[^0-9.])192\.168\.[0-9]{1,3}\.[0-9]{1,3}([^0-9.]|$)
(^|[^0-9.])10\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}([^0-9.]|$)
(^|[^0-9.])172\.(1[6-9]|2[0-9]|3[01])\.[0-9]{1,3}\.[0-9]{1,3}([^0-9.]|$)
[A-Za-z]:[\\/]+Users[\\/]+
-----BEGIN [A-Z ]*PRIVATE KEY-----
(ghp|gho|ghs|ghu)_[A-Za-z0-9]{30,}
github_pat_[A-Za-z0-9_]{30,}
hf_[A-Za-z0-9]{30,}
sk-[A-Za-z0-9_-]{30,}
AKIA[0-9A-Z]{16}
(api|admin)_key[^0-9a-fA-F]{1,6}[0-9a-fA-F]{16,}
EOF

achou=0
if grep -E -f "$tmp/genericos" "$tmp/texto" > "$tmp/achados"; then
  echo "Possível segredo ou dado de rede:" >&2
  cut -c1-160 "$tmp/achados" | head -20 >&2
  achou=1
fi

privados="$raiz/local/padroes-privados.txt"
if [ -f "$privados" ]; then
  grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$privados" > "$tmp/pessoais"
  if [ -s "$tmp/pessoais" ] && grep -i -F -f "$tmp/pessoais" "$tmp/texto" > "$tmp/achados"; then
    echo "Dado pessoal (de local/padroes-privados.txt):" >&2
    cut -c1-160 "$tmp/achados" | head -20 >&2
    achou=1
  fi
fi

exit $achou
