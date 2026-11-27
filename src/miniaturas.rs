//! Geração e cache local de miniaturas de vídeos utilizando FFmpeg em modo somente leitura.
//!
//! Limita automaticamente a quantidade e o tamanho total de arquivos no cache em disco
//! para controlar consumo de armazenamento e memória, sem jamais alterar os vídeos originais.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::erros::{ErroDivisor, Resultado};
use crate::executor_ffmpeg::ExecutorFfmpeg;
use crate::integracao_so::{diretorio_cache_miniaturas, garantir_diretorio};

/// Largura máxima em pixels das miniaturas geradas para exibição na interface.
pub const LARGURA_MAXIMA_MINIATURA_PX: u32 = 240;

/// Limite máximo de arquivos mantidos simultaneamente no cache local de miniaturas.
const LIMITE_ARQUIVOS_CACHE: usize = 200;

/// Limite máximo de tamanho total do cache em disco (30 MB).
const LIMITE_BYTES_CACHE: u64 = 30_000_000;

/// Dados de imagem decodificados em RGBA pronto para envio a qualquer renderizador.
#[derive(Debug, Clone)]
pub struct ImagemRgba {
    /// Largura em pixels.
    pub largura: usize,
    /// Altura em pixels.
    pub altura: usize,
    /// Bytes RGBA8 não pré-multiplicados (`largura * altura * 4`).
    pub pixels: Vec<u8>,
}

/// Gerenciador de cache local de miniaturas em disco.
#[derive(Debug, Clone)]
pub struct GerenciadorMiniaturas {
    diretorio_cache: PathBuf,
}

impl GerenciadorMiniaturas {
    /// Cria o gerenciador utilizando o diretório padrão de cache do sistema operacional.
    pub fn padrao() -> Self {
        Self {
            diretorio_cache: diretorio_cache_miniaturas(),
        }
    }

    /// Cria o gerenciador em um diretório específico (útil para testes).
    pub fn no_diretorio(diretorio_cache: PathBuf) -> Self {
        Self { diretorio_cache }
    }

    /// Retorna o diretório do cache de miniaturas.
    pub fn diretorio_cache(&self) -> &Path {
        &self.diretorio_cache
    }

    /// Obtém do cache ou gera uma miniatura para `arquivo_video` sem alterar o vídeo original.
    pub fn obter_ou_gerar(
        &self,
        executor: &ExecutorFfmpeg,
        arquivo_video: &Path,
        duracao_segundos: f64,
    ) -> Resultado<PathBuf> {
        garantir_diretorio(&self.diretorio_cache)?;

        let chave = calcular_chave_cache(arquivo_video)?;
        let caminho_cache = self.diretorio_cache.join(format!("{chave}.jpg"));

        if caminho_cache.is_file() && fs::metadata(&caminho_cache).is_ok_and(|m| m.len() > 0) {
            return Ok(caminho_cache);
        }

        // Seleciona instante representativo (~10% da duração, limitado entre 0.5s e 5.0s).
        let instante = if duracao_segundos >= 2.0 {
            (duracao_segundos * 0.1).clamp(0.5, 5.0)
        } else {
            0.0
        };

        // Escreve primeiro em arquivo temporário para garantir atomicidade em caso de concorrência.
        let caminho_tmp = self.diretorio_cache.join(format!("{chave}.tmp.jpg"));
        let res = executor.extrair_miniatura(
            arquivo_video,
            instante,
            &caminho_tmp,
            LARGURA_MAXIMA_MINIATURA_PX,
        );

        match res {
            Ok(()) => {
                if fs::rename(&caminho_tmp, &caminho_cache).is_err() {
                    let _ = fs::copy(&caminho_tmp, &caminho_cache);
                    let _ = fs::remove_file(&caminho_tmp);
                }
                let _ = self.aplicar_limites_cache();
                Ok(caminho_cache)
            }
            Err(erro) => {
                let _ = fs::remove_file(&caminho_tmp);
                Err(erro)
            }
        }
    }

    /// Decodifica uma imagem de miniatura do disco em `ImagemRgba`.
    pub fn carregar_rgba(caminho_imagem: &Path) -> Resultado<ImagemRgba> {
        let img = image::open(caminho_imagem).map_err(|erro| ErroDivisor::ErroMiniatura {
            caminho: caminho_imagem.to_path_buf(),
            detalhe: format!("Falha ao decodificar miniatura: {erro}"),
        })?;
        let rgba = img.to_rgba8();
        let (largura, altura) = rgba.dimensions();
        Ok(ImagemRgba {
            largura: largura as usize,
            altura: altura as usize,
            pixels: rgba.into_raw(),
        })
    }

    /// Remove as miniaturas mais antigas caso o cache exceda os limites de arquivos ou bytes.
    pub fn aplicar_limites_cache(&self) -> Resultado<()> {
        if !self.diretorio_cache.is_dir() {
            return Ok(());
        }

        let mut entradas: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
        let mut total_bytes = 0u64;

        let leitor = fs::read_dir(&self.diretorio_cache)
            .map_err(|erro| ErroDivisor::io("leitura do diretório de cache de miniaturas", erro))?;

        for item_res in leitor {
            let Ok(item) = item_res else { continue };
            let caminho = item.path();
            if !caminho.is_file() {
                continue;
            }
            let Ok(meta) = item.metadata() else { continue };
            let tam = meta.len();
            let mod_time = meta.modified().unwrap_or(UNIX_EPOCH);
            total_bytes = total_bytes.saturating_add(tam);
            entradas.push((mod_time, tam, caminho));
        }

        if entradas.len() <= LIMITE_ARQUIVOS_CACHE && total_bytes <= LIMITE_BYTES_CACHE {
            return Ok(());
        }

        // Ordena do mais antigo para o mais recente para remoção LRU aproximada.
        entradas.sort_by_key(|(ts, _, _)| *ts);

        let mut qtd_atual = entradas.len();
        for (_, tam, caminho) in entradas {
            if qtd_atual <= LIMITE_ARQUIVOS_CACHE && total_bytes <= LIMITE_BYTES_CACHE {
                break;
            }
            if fs::remove_file(&caminho).is_ok() {
                qtd_atual = qtd_atual.saturating_sub(1);
                total_bytes = total_bytes.saturating_sub(tam);
            }
        }

        Ok(())
    }

    /// Limpa completamente o cache local de miniaturas.
    pub fn limpar_cache(&self) -> Resultado<usize> {
        if !self.diretorio_cache.is_dir() {
            return Ok(0);
        }
        let mut removidos = 0usize;
        let leitor = fs::read_dir(&self.diretorio_cache)
            .map_err(|erro| ErroDivisor::io("limpeza do cache de miniaturas", erro))?;
        for item_res in leitor {
            let Ok(item) = item_res else { continue };
            if item.path().is_file() && fs::remove_file(item.path()).is_ok() {
                removidos += 1;
            }
        }
        Ok(removidos)
    }
}

/// Calcula uma chave hexadecimal estável (FNV-1a 64-bit) a partir do caminho, tamanho e data
/// de modificação do arquivo de vídeo.
fn calcular_chave_cache(arquivo_video: &Path) -> Resultado<String> {
    let meta = fs::metadata(arquivo_video).map_err(|erro| {
        ErroDivisor::io(
            format!(
                "leitura de metadados para cache de miniatura '{}'",
                arquivo_video.display()
            ),
            erro,
        )
    })?;

    let tamanho = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let atualizar = |h: &mut u64, bytes: &[u8]| {
        for &b in bytes {
            *h ^= u64::from(b);
            *h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };

    atualizar(&mut hash, arquivo_video.to_string_lossy().as_bytes());
    atualizar(&mut hash, &tamanho.to_le_bytes());
    atualizar(&mut hash, &mtime.to_le_bytes());

    Ok(format!("thumb_{hash:016x}"))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn chave_cache_eh_deterministica_e_muda_com_conteudo() {
        let dir = tempfile::tempdir().ok();
        assert!(dir.is_some());
        if let Some(d) = dir {
            let arq = d.path().join("amostra.mp4");
            let _ = fs::write(&arq, b"conteudo_inicial");
            let c1 = calcular_chave_cache(&arq).unwrap_or_default();
            let c2 = calcular_chave_cache(&arq).unwrap_or_default();
            assert_eq!(c1, c2);

            let _ = fs::write(&arq, b"conteudo_maior_alterado");
            let c3 = calcular_chave_cache(&arq).unwrap_or_default();
            assert_ne!(c1, c3);
        }
    }
}
