#!/usr/bin/env bash
# Fonte estável autenticado -> FFmpeg LGPL estático Windows x64. Executar no Linux.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
# $0 pode ser relativo ao diretório de invocação; os cd abaixo não devem invalidá-lo.
script="$root/empacotamento/windows/compilar_ffmpeg.sh"
out=${1:-"$root/build/windows-deps"}
mkdir -p "$out"
out=$(cd "$out" && pwd)
manifest="$root/empacotamento/windows/dependencias.json"
version=$(jq -er '.ffmpeg.version' "$manifest")
[[ "$version" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] || exit 1
mkdir -p "$out/downloads" "$out/licencas" "$out/bin" "$out/fontes"
cd "$out/downloads"
for field in source_url signature_url key_url; do
    url=$(jq -er ".ffmpeg.$field" "$manifest")
    curl --fail --location --retry 3 --proto '=https' --proto-redir '=https' "$url" -o "${url##*/}"
done
keyhome=$(mktemp -d)
trap 'rm -rf "$keyhome"' EXIT
chmod 700 "$keyhome"
gpg --homedir "$keyhome" --batch --import ffmpeg-devel.asc
fingerprint=$(gpg --homedir "$keyhome" --with-colons --fingerprint | awk -F: '$1 == "fpr" {print $10; exit}')
[[ "$fingerprint" == "$(jq -er '.ffmpeg.signing_fingerprint' "$manifest")" ]] || { echo 'Chave inesperada'; exit 1; }
gpg --homedir "$keyhome" --batch --status-fd 1 --verify "ffmpeg-$version.tar.xz.asc" "ffmpeg-$version.tar.xz" > "$out/licencas/assinatura-fonte.txt"
grep -q "VALIDSIG $fingerprint " "$out/licencas/assinatura-fonte.txt"
sha256sum "ffmpeg-$version.tar.xz" > "$out/licencas/SOURCE-SHA256SUMS.txt"
expected=$(jq -r '.ffmpeg.source_sha256 // empty' "$manifest")
if [[ -n "$expected" ]]; then
    printf '%s  %s\n' "$expected" "ffmpeg-$version.tar.xz" | sha256sum -c -
fi
cp "ffmpeg-$version.tar.xz" "ffmpeg-$version.tar.xz.asc" ffmpeg-devel.asc "$out/fontes/"
# Include the exact scripts, manifest, build configuration and MinGW source/licensing.
cp "$script" "$manifest" "$out/fontes/"
cd "$out"
tar -xf "downloads/ffmpeg-$version.tar.xz"
mkdir -p ffmpeg-build
cd ffmpeg-build
args=(--target-os=mingw32 --arch=x86_64 --cpu=x86-64 --enable-cross-compile
 --cross-prefix=x86_64-w64-mingw32- --cc=x86_64-w64-mingw32-gcc-win32
 --enable-static --disable-shared --disable-autodetect --disable-gpl --disable-nonfree
 --disable-version3 --disable-network --disable-unstable --disable-ffplay --disable-doc
 --disable-debug --disable-hwaccels --disable-pthreads --enable-w32threads
 --disable-indevs --enable-indev=lavfi --disable-outdevs
 --disable-encoders --enable-encoder=mjpeg,mpeg4,aac,pcm_s16le,rawvideo,wrapped_avframe
 --extra-cflags=-D_WIN32_WINNT=0x0A00 --extra-ldflags=-static)
printf '%q ' "../ffmpeg-$version/configure" "${args[@]}" > "$out/fontes/configure-command.txt"
printf '\n' >> "$out/fontes/configure-command.txt"
"../ffmpeg-$version/configure" "${args[@]}"
make -j"${JOBS:-2}" ffmpeg.exe ffprobe.exe
cp ffmpeg.exe ffprobe.exe "$out/bin/"
cp config.h ffbuild/config.mak ffbuild/config.log "$out/fontes/"
cp "../ffmpeg-$version/COPYING.LGPLv2.1" "$out/licencas/FFmpeg-LICENSE.txt"
cp "../ffmpeg-$version/LICENSE.md" "$out/licencas/FFmpeg-LICENSE-NOTES.md"
cp "../ffmpeg-$version"/COPYING.* "$out/licencas/"
# Linkable objects + static libraries allow rebuilding/relinking the LGPL programs.
find . -type f \( -name '*.o' -o -name '*.a' \) -print0 | tar --null -T - -czf "$out/fontes/objetos-relink.tar.gz"
{
    echo "FFmpeg $version; LGPL-2.1-or-later; official signed release; no source patches"
    echo "Source: $(jq -r '.ffmpeg.source_url' "$manifest")"
    cat "$out/licencas/SOURCE-SHA256SUMS.txt" "$out/fontes/configure-command.txt"
    x86_64-w64-mingw32-gcc-win32 --version
    dpkg-query -W '*mingw*' 'gcc*' 'binutils*' 2>/dev/null || true
} > "$out/licencas/BUILD-INFO.txt"
mkdir -p "$out/fontes/toolchain" "$out/licencas/toolchain"
# Ubuntu source repositories must be enabled by the workflow. No silent omission.
cd "$out/fontes/toolchain"
apt-get source --download-only "mingw-w64=$(dpkg-query -W -f='${source:Version}' mingw-w64-common)"
cp -r /usr/share/doc/mingw-w64-common "$out/licencas/toolchain/"
cp /usr/share/doc/gcc-mingw-w64-base/copyright "$out/licencas/toolchain/GCC-copyright.txt"
cat > "$out/licencas/THIRD-PARTY-NOTICES.txt" <<'NOTICES'
FFmpeg: LGPL-2.1-or-later. Tools are independent processes, statically built without optional external libraries.
The corresponding official source, signature, configuration and relink objects accompany this package.
MinGW-w64 runtime notices and source are included. GCC runtime uses the GCC Runtime Library Exception (see toolchain/GCC-copyright.txt).
Rust dependencies and their notices/sources are added by preparar_licencas.ps1 before packaging.
This package does not restrict modification or reverse engineering rights granted by third-party licenses.
NOTICES
cd "$out"
# Use tar.gz inside a zip so filenames and permissions survive Windows extraction.
tar -czf fontes-correspondentes.tar.gz fontes
zip -q fontes-correspondentes.zip fontes-correspondentes.tar.gz
(cd bin && sha256sum ffmpeg.exe ffprobe.exe) > "$out/licencas/TOOLS-SHA256SUMS.txt"
python3 - "$manifest" "$out" <<'PY'
import hashlib,json,pathlib,sys
manifest=json.loads(pathlib.Path(sys.argv[1]).read_text()); root=pathlib.Path(sys.argv[2])
manifest['ffmpeg']['source_sha256']=hashlib.sha256((root/'downloads'/f"ffmpeg-{manifest['ffmpeg']['version']}.tar.xz").read_bytes()).hexdigest()
manifest['binaries']={p.name:{'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'bytes':p.stat().st_size} for p in (root/'bin').glob('*.exe')}
(root/'licencas'/'DEPENDENCIAS.json').write_text(json.dumps(manifest,indent=2)+'\n')
PY
