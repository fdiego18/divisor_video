#!/usr/bin/env bash
# Gera pacote Debian/Ubuntu (.deb) utilizando `dpkg-deb`.
set -euo pipefail

RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSAO="2.0.0"
ARQ_DEB="${ARQ_DEB:-$(dpkg --print-architecture 2>/dev/null || echo amd64)}"
DIST_DIR="$RAIZ/dist/debian"
PKG_DIR="$DIST_DIR/divisor-video_${VERSAO}_${ARQ_DEB}"

echo "==> Compilando binários release..."
cd "$RAIZ"
cargo build --release --locked

rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/DEBIAN" \
         "$PKG_DIR/usr/bin" \
         "$PKG_DIR/usr/share/applications" \
         "$PKG_DIR/usr/share/icons/hicolor/scalable/apps" \
         "$PKG_DIR/usr/share/doc/divisor-video"

cp "$RAIZ/target/release/divisor_video" "$PKG_DIR/usr/bin/"
cp "$RAIZ/target/release/divisor_video_gui" "$PKG_DIR/usr/bin/"
chmod 755 "$PKG_DIR/usr/bin/divisor_video" "$PKG_DIR/usr/bin/divisor_video_gui"

cp "$RAIZ/empacotamento/linux/divisor_video.desktop" "$PKG_DIR/usr/share/applications/"
cp "$RAIZ/empacotamento/linux/icone.svg" "$PKG_DIR/usr/share/icons/hicolor/scalable/apps/divisor_video.svg"
cp "$RAIZ/README.md" "$PKG_DIR/usr/share/doc/divisor-video/"
cp "$RAIZ/docs/DISTRIBUICAO_E_LICENCAS.md" "$PKG_DIR/usr/share/doc/divisor-video/"

cat > "$PKG_DIR/DEBIAN/control" << EOF
Package: divisor-video
Version: ${VERSAO}
Section: video
Priority: optional
Architecture: ${ARQ_DEB}
Depends: ffmpeg, libgtk-3-0, libgl1, libxkbcommon0
Maintainer: Equipe de Engenharia <engenharia@example.org>
Description: Divisor de Vídeo sem perda de qualidade (Stream Copy)
 Aplicativo desktop (egui/eframe) e ferramenta CLI em Rust para dividir
 arquivos de vídeo em segmentos respeitando limite de tamanho via FFmpeg
 sem recodificação.
EOF

if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb --build --root-owner-group "$PKG_DIR"
    echo "==> Pacote .deb gerado: ${PKG_DIR}.deb"
else
    echo "==> Estrutura Debian montada em $PKG_DIR (execute 'dpkg-deb --build' em Debian/Ubuntu)."
fi
