#!/usr/bin/env bash
# Empacotamento Linux portátil (.tar.gz) e AppImage (opcional com `appimagetool`).
# Inclui opcionalmente FFmpeg e FFprobe em `ferramentas/` para distribuição autônoma.
set -euo pipefail

RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSAO="2.0.0"
ARQ="$(uname -m)"
DIST_DIR="$RAIZ/dist/linux"
PORTATIL_DIR="$DIST_DIR/divisor_video-${VERSAO}-linux-${ARQ}"

echo "==> Compilando binários release..."
cd "$RAIZ"
cargo build --release --locked

echo "==> Montando distribuição portátil em $PORTATIL_DIR..."
rm -rf "$PORTATIL_DIR"
mkdir -p "$PORTATIL_DIR/ferramentas" "$PORTATIL_DIR/licencas"

cp "$RAIZ/target/release/divisor_video" "$PORTATIL_DIR/"
cp "$RAIZ/target/release/divisor_video_gui" "$PORTATIL_DIR/"
cp "$RAIZ/README.md" "$PORTATIL_DIR/"
cp "$RAIZ/docs/DISTRIBUICAO_E_LICENCAS.md" "$PORTATIL_DIR/licencas/"

if [[ "${INCLUIR_FFMPEG:-0}" == "1" ]]; then
    FFMPEG_BIN="${FFMPEG_CAMINHO:-$(command -v ffmpeg || true)}"
    FFPROBE_BIN="${FFPROBE_CAMINHO:-$(command -v ffprobe || true)}"
    if [[ -n "$FFMPEG_BIN" && -n "$FFPROBE_BIN" ]]; then
        echo "==> Incluindo FFmpeg e FFprobe em ferramentas/ (verifique obrigações LGPL/GPL em licencas/DISTRIBUICAO_E_LICENCAS.md)..."
        cp -L "$FFMPEG_BIN" "$PORTATIL_DIR/ferramentas/ffmpeg"
        cp -L "$FFPROBE_BIN" "$PORTATIL_DIR/ferramentas/ffprobe"
        chmod +x "$PORTATIL_DIR/ferramentas/ffmpeg" "$PORTATIL_DIR/ferramentas/ffprobe"
    else
        echo "AVISO: INCLUIR_FFMPEG=1 definido, mas ffmpeg/ffprobe não encontrados." >&2
    fi
fi

mkdir -p "$DIST_DIR"
tar -C "$DIST_DIR" -czf "$DIST_DIR/divisor_video-${VERSAO}-linux-${ARQ}.tar.gz" "$(basename "$PORTATIL_DIR")"
echo "==> Pacote portátil gerado: $DIST_DIR/divisor_video-${VERSAO}-linux-${ARQ}.tar.gz"

# Monta AppDir (pronto para AppImage)
APPDIR="$DIST_DIR/DivisorVideo.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin/ferramentas" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/scalable/apps"

cp "$RAIZ/target/release/divisor_video_gui" "$APPDIR/usr/bin/"
cp "$RAIZ/target/release/divisor_video" "$APPDIR/usr/bin/"
cp "$RAIZ/empacotamento/linux/divisor_video.desktop" "$APPDIR/divisor_video.desktop"
cp "$RAIZ/empacotamento/linux/divisor_video.desktop" "$APPDIR/usr/share/applications/"
cp "$RAIZ/empacotamento/linux/icone.svg" "$APPDIR/divisor_video.svg"
cp "$RAIZ/empacotamento/linux/icone.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/divisor_video.svg"

if [[ -f "$PORTATIL_DIR/ferramentas/ffmpeg" ]]; then
    cp "$PORTATIL_DIR/ferramentas/ffmpeg" "$APPDIR/usr/bin/ferramentas/"
    cp "$PORTATIL_DIR/ferramentas/ffprobe" "$APPDIR/usr/bin/ferramentas/"
fi

cat > "$APPDIR/AppRun" << 'EOF'
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "${0}")")"
exec "$HERE/usr/bin/divisor_video_gui" "$@"
EOF
chmod +x "$APPDIR/AppRun"

if command -v appimagetool >/dev/null 2>&1; then
    ARCH="$ARQ" appimagetool "$APPDIR" "$DIST_DIR/DivisorVideo-${VERSAO}-${ARQ}.AppImage"
    echo "==> AppImage gerado: $DIST_DIR/DivisorVideo-${VERSAO}-${ARQ}.AppImage"
else
    echo "==> AppDir pronto em $APPDIR (instale 'appimagetool' para empacotar o arquivo .AppImage final)."
fi
