#!/usr/bin/env bash
# Gera o .deb do Helm via Docker (fallback quando a máquina remota não está
# acessível). Padrão: linux/amd64 — em Apple Silicon roda sob emulação
# (Rosetta/QEMU), funcional porém lento; prefira scripts/build-deb-remote.sh.
#
# Uso: scripts/build-deb.sh
#   HELM_DEB_PLATFORM=linux/arm64  para .deb arm64 nativo no Apple Silicon
set -euo pipefail

PLATFORM="${HELM_DEB_PLATFORM:-linux/amd64}"
LOCAL_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAG="helm-deb-builder:$(echo "$PLATFORM" | tr '/' '-')"

if [ ! -f "$LOCAL_ROOT/package.json" ]; then
  echo "erro: package.json não encontrado — rode após o scaffold Tauri (Fase 0)." >&2
  exit 1
fi

echo "==> Build da imagem ($PLATFORM)"
docker build --platform "$PLATFORM" -t "$TAG" -f "$LOCAL_ROOT/docker/Dockerfile.deb" "$LOCAL_ROOT/docker"

echo "==> Compilando (caches em volumes nomeados)"
mkdir -p "$LOCAL_ROOT/dist"
docker run --rm --platform "$PLATFORM" \
  -v "$LOCAL_ROOT":/app \
  -v "helm-cargo-registry-${PLATFORM##*/}":/root/.cargo/registry \
  -v "helm-node-modules-${PLATFORM##*/}":/app/node_modules \
  -v "helm-target-${PLATFORM##*/}":/app/src-tauri/target \
  -w /app "$TAG" \
  bash -c 'npm ci && npm run tauri build -- --bundles deb \
           && cp src-tauri/target/release/bundle/deb/*.deb /app/dist/'

echo "==> OK: $(ls "$LOCAL_ROOT"/dist/*.deb | tail -1)"
