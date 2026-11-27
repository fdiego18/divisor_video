"""Contrato de entrega entre jobs; Python é usado somente no ambiente de build."""

import hashlib
import json
from pathlib import Path
import re
import sys
import zipfile


def validar(raiz):
    raiz = Path(raiz)
    obrigatorios = (
        "bin/ffmpeg.exe", "bin/ffprobe.exe", "licencas/DEPENDENCIAS.json",
        "licencas/FFmpeg-LICENSE.txt", "licencas/THIRD-PARTY-NOTICES.txt",
        "licencas/BUILD-INFO.txt", "licencas/assinatura-fonte.txt",
        "licencas/SOURCE-SHA256SUMS.txt", "licencas/TOOLS-SHA256SUMS.txt",
        "fontes-correspondentes.zip", "compilacao-ffmpeg.log",
    )
    for relativo in obrigatorios:
        arquivo = raiz / relativo
        if not arquivo.is_file() or arquivo.stat().st_size == 0:
            raise ValueError(f"Entrega FFmpeg incompleta: {arquivo}")
    manifesto = json.loads((raiz / "licencas/DEPENDENCIAS.json").read_text(encoding="utf-8"))
    for nome in ("ffmpeg.exe", "ffprobe.exe"):
        esperado = manifesto["binaries"][nome]
        arquivo = raiz / "bin" / nome
        if not re.fullmatch(r"[0-9a-f]{64}", esperado["sha256"]):
            raise ValueError(f"SHA-256 inválido no manifesto: {nome}")
        with arquivo.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        if digest != esperado["sha256"] or arquivo.stat().st_size != esperado["bytes"]:
            raise ValueError(f"Binário divergente do manifesto autenticado: {nome}")
    with zipfile.ZipFile(raiz / "fontes-correspondentes.zip") as fontes:
        if "fontes-correspondentes.tar.gz" not in fontes.namelist() or fontes.testzip():
            raise ValueError("Arquivo de fontes correspondentes incompleto ou corrompido")


if __name__ == "__main__":
    validar(sys.argv[1])
    print("Entrega FFmpeg completa; tamanhos, SHA-256 e arquivo de fontes verificados.")
