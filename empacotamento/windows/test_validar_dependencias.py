"""Regressão do contrato de artefatos, sem recompilar nem substituir o FFmpeg."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import zipfile

from validar_dependencias import validar


@unittest.skipUnless(os.name == "posix" and shutil.which("jq"), "Receita FFmpeg executada no Linux com jq")
class ReceitaFfmpeg(unittest.TestCase):
    def test_invocacao_relativa_preserva_script_apos_mudar_diretorio(self):
        # Simula apenas aquisição/autenticação; para antes de compilar. Não usa rede.
        with tempfile.TemporaryDirectory(prefix="receita com espaços ") as temp:
            raiz = Path(temp)
            scripts = raiz / "empacotamento/windows"
            scripts.mkdir(parents=True)
            origem = Path(__file__).parent
            for nome in ("compilar_ffmpeg.sh", "dependencias.json"):
                shutil.copyfile(origem / nome, scripts / nome)
            wrapper = r'''
curl() { printf 'fixture' > "${@: -1}"; }
gpg() {
    case "$*" in
        *--with-colons*) printf 'fpr:::::::::FCF986EA15E6E293A5644F10B4322F04D67658D8:\n' ;;
        *--verify*) printf '[GNUPG:] VALIDSIG FCF986EA15E6E293A5644F10B4322F04D67658D8 fixture\n' ;;
    esac
}
tar() { exit 73; }
export -f curl gpg tar
bash empacotamento/windows/compilar_ffmpeg.sh
'''
            resultado = subprocess.run(["bash", "-c", wrapper], cwd=raiz, capture_output=True, text=True)
            self.assertEqual(resultado.returncode, 73, resultado.stderr)
            copia = raiz / "build/windows-deps/fontes/compilar_ffmpeg.sh"
            self.assertEqual(copia.read_bytes(), (scripts / "compilar_ffmpeg.sh").read_bytes())

    def test_pipefail_propaga_falha_apesar_do_tee(self):
        resultado = subprocess.run(
            ["bash", "-euo", "pipefail", "-c", "(exit 23) | tee /dev/null"],
            capture_output=True,
        )
        self.assertEqual(resultado.returncode, 23)


class EntregaFfmpeg(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="entrega com espaços ")
        self.addCleanup(self.temp.cleanup)
        self.raiz = Path(self.temp.name)
        (self.raiz / "bin").mkdir()
        (self.raiz / "licencas").mkdir()
        binaries = {}
        for nome in ("ffmpeg.exe", "ffprobe.exe"):
            dados = b"fixture de transporte " + nome.encode()
            (self.raiz / "bin" / nome).write_bytes(dados)
            binaries[nome] = {"sha256": hashlib.sha256(dados).hexdigest(), "bytes": len(dados)}
        (self.raiz / "licencas/DEPENDENCIAS.json").write_text(json.dumps({"binaries": binaries}))
        for nome in ("FFmpeg-LICENSE.txt", "THIRD-PARTY-NOTICES.txt", "BUILD-INFO.txt",
                     "assinatura-fonte.txt", "SOURCE-SHA256SUMS.txt", "TOOLS-SHA256SUMS.txt"):
            (self.raiz / "licencas" / nome).write_text("fixture")
        (self.raiz / "compilacao-ffmpeg.log").write_text("fixture")
        with zipfile.ZipFile(self.raiz / "fontes-correspondentes.zip", "w") as fontes:
            fontes.writestr("fontes-correspondentes.tar.gz", b"fixture")

    def test_entrega_completa_e_transporte_preservam_estrutura(self):
        validar(self.raiz)
        arquivo = self.raiz / "entrega.tar"
        with tarfile.open(arquivo, "w") as tar:
            for nome in ("bin", "licencas", "fontes-correspondentes.zip", "compilacao-ffmpeg.log"):
                tar.add(self.raiz / nome, arcname=nome)
        destino = self.raiz / "extraído com espaços"
        with tarfile.open(arquivo) as tar:
            tar.extractall(destino, filter="data")
        validar(destino)

    def test_manifesto_ausente_reproduz_falha_do_job(self):
        (self.raiz / "licencas/DEPENDENCIAS.json").unlink()
        with self.assertRaisesRegex(ValueError, "DEPENDENCIAS.json"):
            validar(self.raiz)

    def test_binario_ausente_ou_corrompido_e_bloqueado(self):
        tool = self.raiz / "bin/ffprobe.exe"
        tool.write_bytes(b"adulterado")
        with self.assertRaisesRegex(ValueError, "divergente"):
            validar(self.raiz)
        tool.unlink()
        with self.assertRaisesRegex(ValueError, "incompleta"):
            validar(self.raiz)

    def test_fontes_incompletos_sao_bloqueados(self):
        with zipfile.ZipFile(self.raiz / "fontes-correspondentes.zip", "w") as fontes:
            fontes.writestr("outro.txt", "fixture")
        with self.assertRaisesRegex(ValueError, "fontes correspondentes"):
            validar(self.raiz)


if __name__ == "__main__":
    unittest.main()
