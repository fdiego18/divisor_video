#!/usr/bin/env bash
# Empacotamento macOS (.app e .dmg) com suporte a Apple Silicon (aarch64-apple-darwin)
# e Intel (x86_64-apple-darwin), incluindo suporte a binário universal via `lipo`.
set -euo pipefail

RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSAO="2.0.0"
ARQ_ALVO="${1:-native}" # opcoes: native | arm64 | x86_64 | universal
DIST_DIR="$RAIZ/dist/macos"
APP_BUNDLE="$DIST_DIR/Divisor de Vídeo.app"

cd "$RAIZ"
mkdir -p "$DIST_DIR"

compilar_target() {
    local target="$1"
    rustup target add "$target" >/dev/null 2>&1 || true
    cargo build --release --locked --target "$target"
}

GUI_BIN=""
CLI_BIN=""

case "$ARQ_ALVO" in
    arm64)
        compilar_target "aarch64-apple-darwin"
        GUI_BIN="$RAIZ/target/aarch64-apple-darwin/release/divisor_video_gui"
        CLI_BIN="$RAIZ/target/aarch64-apple-darwin/release/divisor_video"
        ;;
    x86_64)
        compilar_target "x86_64-apple-darwin"
        GUI_BIN="$RAIZ/target/x86_64-apple-darwin/release/divisor_video_gui"
        CLI_BIN="$RAIZ/target/x86_64-apple-darwin/release/divisor_video"
        ;;
    universal)
        compilar_target "aarch64-apple-darwin"
        compilar_target "x86_64-apple-darwin"
        GUI_BIN="$DIST_DIR/divisor_video_gui_universal"
        CLI_BIN="$DIST_DIR/divisor_video_universal"
        lipo -create \
            "$RAIZ/target/aarch64-apple-darwin/release/divisor_video_gui" \
            "$RAIZ/target/x86_64-apple-darwin/release/divisor_video_gui" \
            -output "$GUI_BIN"
        lipo -create \
            "$RAIZ/target/aarch64-apple-darwin/release/divisor_video" \
            "$RAIZ/target/x86_64-apple-darwin/release/divisor_video" \
            -output "$CLI_BIN"
        ;;
    native)
        cargo build --release --locked
        GUI_BIN="$RAIZ/target/release/divisor_video_gui"
        CLI_BIN="$RAIZ/target/release/divisor_video"
        ;;
    *)
        echo "Uso: $0 [native|arm64|x86_64|universal]" >&2
        exit 2
        ;;
esac

echo "==> Montando pacote .app em $APP_BUNDLE..."
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS/ferramentas" "$APP_BUNDLE/Contents/Resources"

cp "$GUI_BIN" "$APP_BUNDLE/Contents/MacOS/divisor_video_gui"
cp "$CLI_BIN" "$APP_BUNDLE/Contents/MacOS/divisor_video"
chmod +x "$APP_BUNDLE/Contents/MacOS/divisor_video_gui" "$APP_BUNDLE/Contents/MacOS/divisor_video"

if [[ -n "${FFMPEG_BIN:-}" && -f "${FFMPEG_BIN}" ]]; then
    cp "$FFMPEG_BIN" "$APP_BUNDLE/Contents/MacOS/ferramentas/ffmpeg"
    chmod +x "$APP_BUNDLE/Contents/MacOS/ferramentas/ffmpeg"
fi
if [[ -n "${FFPROBE_BIN:-}" && -f "${FFPROBE_BIN}" ]]; then
    cp "$FFPROBE_BIN" "$APP_BUNDLE/Contents/MacOS/ferramentas/ffprobe"
    chmod +x "$APP_BUNDLE/Contents/MacOS/ferramentas/ffprobe"
fi

cat > "$APP_BUNDLE/Contents/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Divisor de Vídeo</string>
    <key>CFBundleDisplayName</key>
    <string>Divisor de Vídeo</string>
    <key>CFBundleIdentifier</key>
    <string>br.org.divisorvideo.app</string>
    <key>CFBundleVersion</key>
    <string>${VERSAO}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSAO}</string>
    <key>CFBundleExecutable</key>
    <string>divisor_video_gui</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
</dict>
</plist>
EOF

DMG_FINAL="$DIST_DIR/DivisorVideo-${VERSAO}-macos-${ARQ_ALVO}.dmg"
if command -v hdiutil >/dev/null 2>&1; then
    rm -f "$DMG_FINAL"
    hdiutil create -volname "Divisor de Vídeo" -srcfolder "$APP_BUNDLE" -ov -format UDZO "$DMG_FINAL"
    echo "==> Imagem DMG gerada: $DMG_FINAL"
else
    echo "==> Bundle .app pronto em $APP_BUNDLE (execute em macOS com hdiutil para criar o .dmg)."
fi
