//! Descoberta de arquivos e processamento paralelo em lote com concorrência controlada via Rayon.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rayon::prelude::*;
use tracing::{error, info};

use crate::configuracao::Configuracao;
use crate::divisor_video::{RelatorioVideo, StatusProcessamentoVideo, dividir_arquivo_video};
use crate::erros::{ErroDivisor, Resultado};
use crate::executor_ffmpeg::ExecutorFfmpeg;
use crate::progresso::{GerenciadorProgresso, formatar_tamanho_bytes, formatar_velocidade};

/// Extensões de contêineres de vídeo reconhecidas durante varredura de diretórios.
const EXTENSOES_VIDEO_SUPORTADAS: &[&str] = &[
    "mp4", "mkv", "mov", "avi", "webm", "m4v", "flv", "ts", "m2ts", "mts", "mpeg", "mpg", "wmv",
    "ogv", "3gp", "3g2", "vob",
];

/// Resultado individual de um item dentro do processamento em lote.
#[derive(Debug)]
pub enum ResultadoItemLote {
    /// Vídeo processado (dividido ou ignorado abaixo do limite).
    Sucesso(RelatorioVideo),
    /// Erro durante o processamento deste vídeo específico.
    Falha {
        /// Caminho do arquivo que falhou.
        arquivo: PathBuf,
        /// Erro detalhado ocorrido.
        erro: ErroDivisor,
    },
}

/// Resumo consolidado de toda a execução em lote.
#[derive(Debug)]
pub struct ResumoLote {
    /// Resultados de cada arquivo descoberto.
    pub itens: Vec<ResultadoItemLote>,
    /// Tempo total decorrido do lote completo.
    pub tempo_total: Duration,
    /// Total de bytes de vídeos originais processados com sucesso.
    pub bytes_originais_totais: u64,
    /// Indica se o lote foi interrompido por cancelamento do usuário.
    pub cancelado: bool,
}

impl ResumoLote {
    /// Quantidade de vídeos efetivamente divididos.
    pub fn quantidade_divididos(&self) -> usize {
        self.itens
            .iter()
            .filter(|i| {
                matches!(
                    i,
                    ResultadoItemLote::Sucesso(RelatorioVideo {
                        status: StatusProcessamentoVideo::Dividido { .. },
                        ..
                    })
                )
            })
            .count()
    }

    /// Quantidade de vídeos ignorados por já estarem dentro do limite.
    pub fn quantidade_ignorados(&self) -> usize {
        self.itens
            .iter()
            .filter(|i| {
                matches!(
                    i,
                    ResultadoItemLote::Sucesso(RelatorioVideo {
                        status: StatusProcessamentoVideo::IgnoradoAbaixoDoLimite { .. },
                        ..
                    })
                )
            })
            .count()
    }

    /// Quantidade de vídeos que terminaram em erro.
    pub fn quantidade_falhas(&self) -> usize {
        self.itens
            .iter()
            .filter(|i| matches!(i, ResultadoItemLote::Falha { .. }))
            .count()
    }
}

/// Descobre a lista ordenada de arquivos de vídeo a partir de um arquivo único ou diretório.
pub fn descobrir_videos(config: &Configuracao) -> Resultado<Vec<PathBuf>> {
    let entrada = &config.entrada;
    if !entrada.exists() {
        return Err(ErroDivisor::ArquivoNaoEncontrado {
            caminho: entrada.clone(),
        });
    }

    if entrada.is_file() {
        return Ok(vec![entrada.clone()]);
    }

    let saida_canonica = fs::canonicalize(&config.diretorio_saida).ok();
    let mut arquivos = Vec::new();
    coletar_videos_diretorio(
        entrada,
        config.recursivo,
        saida_canonica.as_deref(),
        &mut arquivos,
    )?;

    arquivos.sort();
    Ok(arquivos)
}

/// Varre um diretório coletando arquivos com extensão de vídeo suportada.
fn coletar_videos_diretorio(
    dir: &Path,
    recursivo: bool,
    dir_saida_ignorar: Option<&Path>,
    acumulador: &mut Vec<PathBuf>,
) -> Resultado<()> {
    let entradas = fs::read_dir(dir).map_err(|erro| {
        ErroDivisor::io(
            format!("leitura do diretório de entrada '{}'", dir.display()),
            erro,
        )
    })?;

    for entrada_res in entradas {
        let entrada = entrada_res
            .map_err(|erro| ErroDivisor::io("iteração de diretório de entrada", erro))?;
        let caminho = entrada.path();

        if caminho.is_dir() {
            if recursivo {
                // Evita reprocessar o próprio diretório de saída caso esteja dentro da entrada.
                if let Some(ignorar) = dir_saida_ignorar
                    && fs::canonicalize(&caminho).ok().as_deref() == Some(ignorar)
                {
                    continue;
                }
                // Ignora diretórios temporários ocultos.
                if caminho
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(".divisor_tmp_"))
                {
                    continue;
                }
                coletar_videos_diretorio(&caminho, recursivo, dir_saida_ignorar, acumulador)?;
            }
        } else if caminho.is_file() && possui_extensao_video(&caminho) {
            acumulador.push(caminho);
        }
    }

    Ok(())
}

/// Verifica se o arquivo possui uma extensão de vídeo reconhecida (case-insensitive).
pub fn possui_extensao_video(caminho: &Path) -> bool {
    caminho
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|ext| EXTENSOES_VIDEO_SUPORTADAS.contains(&ext.as_str()))
}

/// Executa o processamento de todos os vídeos descobertos utilizando um pool Rayon
/// dedicado com concorrência rigorosamente controlada.
pub fn executar_lote(
    config: &Configuracao,
    executor: &ExecutorFfmpeg,
    cancelamento: &Arc<AtomicBool>,
) -> Resultado<ResumoLote> {
    let inicio_lote = Instant::now();
    let videos = descobrir_videos(config)?;

    info!(
        total_videos = videos.len(),
        concorrencia = config.concorrencia,
        limite_bytes = config.tamanho_maximo_bytes,
        alvo_bytes = config.tamanho_alvo_bytes,
        "Iniciando processamento em lote"
    );

    if videos.is_empty() {
        return Ok(ResumoLote {
            itens: Vec::new(),
            tempo_total: inicio_lote.elapsed(),
            bytes_originais_totais: 0,
            cancelado: false,
        });
    }

    let progresso = GerenciadorProgresso::novo(videos.len(), config.silencioso);
    let num_threads = config.concorrencia.min(videos.len()).max(1);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .thread_name(|idx| format!("divisor-worker-{idx}"))
        .build()
        .map_err(|erro| ErroDivisor::ConfiguracaoInvalida {
            detalhe: format!("Falha ao criar pool paralelo Rayon: {erro}"),
        })?;

    let itens: Vec<ResultadoItemLote> = pool.install(|| {
        videos
            .par_iter()
            .map(|caminho| {
                if cancelamento.load(Ordering::Relaxed) {
                    progresso.concluir_video_no_lote();
                    return ResultadoItemLote::Falha {
                        arquivo: caminho.clone(),
                        erro: ErroDivisor::CanceladoPeloUsuario,
                    };
                }

                let resultado =
                    dividir_arquivo_video(caminho, config, executor, &progresso, cancelamento);
                progresso.concluir_video_no_lote();

                match resultado {
                    Ok(relatorio) => ResultadoItemLote::Sucesso(relatorio),
                    Err(erro) => {
                        if !matches!(erro, ErroDivisor::CanceladoPeloUsuario) {
                            error!(arquivo = %caminho.display(), %erro, "Falha ao processar vídeo");
                        }
                        ResultadoItemLote::Falha {
                            arquivo: caminho.clone(),
                            erro,
                        }
                    }
                }
            })
            .collect()
    });

    let cancelado = cancelamento.load(Ordering::Relaxed);
    progresso.finalizar(if cancelado { "cancelado" } else { "concluído" });

    let bytes_originais_totais = itens
        .iter()
        .filter_map(|item| match item {
            ResultadoItemLote::Sucesso(rel) => Some(rel.tamanho_original_bytes),
            ResultadoItemLote::Falha { .. } => None,
        })
        .sum();

    Ok(ResumoLote {
        itens,
        tempo_total: inicio_lote.elapsed(),
        bytes_originais_totais,
        cancelado,
    })
}

/// Imprime um relatório legível e detalhado no `stdout` contendo tamanho original,
/// tamanho de cada segmento, velocidade e tempo decorrido.
pub fn imprimir_resumo_lote(resumo: &ResumoLote, limite_maximo_bytes: u64) {
    println!("\n=== Resumo do Processamento ===");

    if resumo.itens.is_empty() {
        println!("Nenhum arquivo de vídeo compatível encontrado na entrada informada.");
        return;
    }

    for item in &resumo.itens {
        match item {
            ResultadoItemLote::Sucesso(relatorio) => {
                println!("\nArquivo: {}", relatorio.arquivo_original.display());
                println!(
                    "  Tamanho original : {}",
                    formatar_tamanho_bytes(relatorio.tamanho_original_bytes)
                );
                println!(
                    "  Tempo decorrido  : {:.2}s | Velocidade: {}",
                    relatorio.tempo_decorrido.as_secs_f64(),
                    formatar_velocidade(
                        relatorio.tamanho_original_bytes,
                        relatorio.tempo_decorrido
                    )
                );

                match &relatorio.status {
                    StatusProcessamentoVideo::IgnoradoAbaixoDoLimite { limite_bytes } => {
                        println!(
                            "  Status           : Ignorado (já está dentro do limite de {})",
                            formatar_tamanho_bytes(*limite_bytes)
                        );
                    }
                    StatusProcessamentoVideo::Dividido {
                        diretorio_destino,
                        segmentos,
                        tentativas,
                        eh_vbr,
                        ..
                    } => {
                        println!(
                            "  Status           : Dividido em {} parte(s) [{} | {} tentativa(s)]",
                            segmentos.len(),
                            if *eh_vbr { "VBR" } else { "CBR" },
                            tentativas
                        );
                        println!("  Pasta de saída   : {}", diretorio_destino.display());
                        for seg in segmentos {
                            #[allow(clippy::cast_precision_loss)]
                            let aproveitamento = (seg.tamanho_bytes as f64
                                / limite_maximo_bytes.max(1) as f64)
                                * 100.0;
                            let nome_seg = seg.caminho.file_name().map_or_else(
                                || seg.caminho.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            );
                            println!(
                                "    - {:<32} | {:<28} | {:>5.1}% do limite | {:.2}s",
                                nome_seg,
                                formatar_tamanho_bytes(seg.tamanho_bytes),
                                aproveitamento,
                                seg.duracao_segundos
                            );
                        }
                    }
                }
            }
            ResultadoItemLote::Falha { arquivo, erro } => {
                println!("\nArquivo: {}", arquivo.display());
                println!("  Status           : ERRO");
                println!("  Detalhe          : {erro}");
            }
        }
    }

    println!("\n--- Totais ---");
    println!(
        "Divididos: {} | Ignorados: {} | Falhas: {} | Tempo total: {:.2}s | Velocidade média: {}",
        resumo.quantidade_divididos(),
        resumo.quantidade_ignorados(),
        resumo.quantidade_falhas(),
        resumo.tempo_total.as_secs_f64(),
        formatar_velocidade(resumo.bytes_originais_totais, resumo.tempo_total)
    );
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn reconhece_extensoes_de_video_case_insensitive() {
        assert!(possui_extensao_video(Path::new("filme.mp4")));
        assert!(possui_extensao_video(Path::new("FILME.MKV")));
        assert!(possui_extensao_video(Path::new("clipe.WebM")));
        assert!(possui_extensao_video(Path::new("captura.MOV")));
        assert!(possui_extensao_video(Path::new("antigo.avi")));
        assert!(!possui_extensao_video(Path::new("documento.pdf")));
        assert!(!possui_extensao_video(Path::new("sem_extensao")));
    }
}
