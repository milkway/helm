#!/usr/bin/env bash
# Gera o .deb (amd64) do Helm compilando na máquina remota "prompt"
# (Ubuntu 22.04 x86_64, glibc 2.35 — mesmo alvo do CI).
#
# Uso: scripts/build-deb-remote.sh
#   HELM_BUILD_HOST=<host-ssh>  para usar outra máquina (padrão: prompt)
#
# O artefato volta para ./dist/ no final.
set -euo pipefail

REMOTE="${HELM_BUILD_HOST:-prompt}"
RDIR="helm-build"
LOCAL_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [ ! -f "$LOCAL_ROOT/package.json" ]; then
  echo "erro: package.json não encontrado — rode após o scaffold Tauri (Fase 0)." >&2
  exit 1
fi

echo "==> Sincronizando fontes para $REMOTE:~/$RDIR"
rsync -az --delete \
  --exclude .git \
  --exclude node_modules \
  --exclude dist \
  --exclude 'src-tauri/target' \
  "$LOCAL_ROOT"/ "$REMOTE:$RDIR/"

echo "==> Compilando no host remoto"
ssh "$REMOTE" bash -s <<'REMOTE_SCRIPT'
set -euo pipefail
cd ~/helm-build

# Dependências de sistema do Tauri 2 no Linux (instalação exige sudo — feita uma vez).
DEPS=(build-essential pkg-config curl file libssl-dev libgtk-3-dev
      libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev)
MISSING=()
for d in "${DEPS[@]}"; do dpkg -s "$d" >/dev/null 2>&1 || MISSING+=("$d"); done
if [ "${#MISSING[@]}" -gt 0 ]; then
  echo "Dependências ausentes. Rode uma vez na máquina remota:" >&2
  echo "  sudo apt-get update && sudo apt-get install -y ${MISSING[*]}" >&2
  exit 1
fi

# Rust via rustup (instala sem sudo, uma vez).
if ! command -v cargo >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  echo "==> Instalando Rust (rustup)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
export PATH="$HOME/.cargo/bin:$PATH"

npm ci
npm run tauri build -- --bundles deb
ls -lh src-tauri/target/release/bundle/deb/*.deb
REMOTE_SCRIPT

echo "==> Trazendo artefato para ./dist/"
mkdir -p "$LOCAL_ROOT/dist"
rsync -av "$REMOTE:$RDIR/src-tauri/target/release/bundle/deb/*.deb" "$LOCAL_ROOT/dist/"
echo "==> OK: $(ls "$LOCAL_ROOT"/dist/*.deb | tail -1)"
