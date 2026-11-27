//! Validação rigorosa de tamanho, preservação de qualidade/codecs, integridade de pacotes
//! e continuidade temporal dos segmentos gerados.

use std::fs;
use std::path::{Path, PathBuf};

use tracing::debug;

use crate::analisador_video::{MapaVideo, MetadadosVideo, analisar_metadados};
use crate::erros::{ErroDivisor, Resultado};
use crate::executor_ffmpeg::ExecutorFfmpeg;

/// Resultado detalhado da validação de um segmento individual.
#[derive(Debug, Clone)]
pub struct SegmentoValidado {
    /// Índice do segmento (base 1).
    pub indice: usize,
    /// Caminho atual do arquivo do segmento.
    pub caminho: PathBuf,
    /// Tamanho real verificado em disco (em bytes).
    pub tamanho_bytes: u64,
    /// Duração real medida pelo FFprobe (em segundos).
    pub duracao_segundos: f64,
    /// Quantidade de pacotes de vídeo no segmento.
    pub pacotes_video: u64,
    /// Quantidade de pacotes de áudio (todas as faixas) no segmento.
    pub pacotes_audio: u64,
    /// Confirmação de que o primeiro pacote de vídeo é um keyframe.
    pub inicia_em_keyframe: bool,
}

/// Resumo consolidado da validação global do conjunto de segmentos.
#[derive(Debug, Clone)]
pub struct RelatorioValidacaoConjunto {
    /// Quantidade de segmentos validados.
    pub total_segmentos: usize,
    /// Soma das durações de todos os segmentos (em segundos).
    pub duracao_total_segmentos: f64,
    /// Soma dos pacotes de vídeo de todos os segmentos.
    pub total_pacotes_video_segmentos: u64,
    /// Soma dos pacotes de áudio de todos os segmentos.
    pub total_pacotes_audio_segmentos: u64,
    /// Maior tamanho de arquivo entre os segmentos gerados (em bytes).
    pub maior_segmento_bytes: u64,
}

/// Valida individualmente um segmento gerado em relação ao vídeo original e ao limite máximo.
///
/// Regra inviolável: qualquer segmento com `tamanho_bytes > limite_maximo_bytes` é rejeitado.
pub fn validar_segmento(
    executor: &ExecutorFfmpeg,
    original: &MetadadosVideo,
    caminho_segmento: &Path,
    indice: usize,
    limite_maximo_bytes: u64,
) -> Resultado<SegmentoValidado> {
    let tamanho_bytes = fs::metadata(caminho_segmento)
        .map_err(|erro| {
            ErroDivisor::io(
                format!(
                    "leitura de tamanho do segmento '{}'",
                    caminho_segmento.display()
                ),
                erro,
            )
        })?
        .len();

    if tamanho_bytes == 0 {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: format!(
                "Segmento {indice} ('{}') foi gerado vazio (0 bytes).",
                caminho_segmento.display()
            ),
        });
    }

    if tamanho_bytes > limite_maximo_bytes {
        return Err(ErroDivisor::SegmentoExcedeuLimite {
            indice,
            tamanho_bytes,
            limite_bytes: limite_maximo_bytes,
        });
    }

    let meta_seg = analisar_metadados(executor, caminho_segmento)?;
    comparar_parametros_stream(original, &meta_seg, indice)?;

    let resumo_pacotes = executor.inspecionar_pacotes_segmento(caminho_segmento, None)?;

    if resumo_pacotes.pacotes_video == 0 {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: format!("Segmento {indice} não contém pacotes de vídeo."),
        });
    }

    if !resumo_pacotes.primeiro_pacote_video_eh_keyframe {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: format!(
                "Segmento {indice} não inicia em um keyframe válido, comprometendo a reprodução independente."
            ),
        });
    }

    let duracao_calculada = if meta_seg.duracao_segundos > 0.0 {
        meta_seg.duracao_segundos
    } else if let (Some(ini), Some(fim)) = (
        resumo_pacotes.primeiro_pts_video,
        resumo_pacotes.ultimo_pts_video,
    ) {
        (fim - ini).max(0.0)
    } else {
        0.0
    };

    debug!(
        indice,
        tamanho_bytes,
        duracao_segundos = duracao_calculada,
        pacotes_video = resumo_pacotes.pacotes_video,
        pacotes_audio = resumo_pacotes.pacotes_audio,
        "Segmento validado com sucesso"
    );

    Ok(SegmentoValidado {
        indice,
        caminho: caminho_segmento.to_path_buf(),
        tamanho_bytes,
        duracao_segundos: duracao_calculada,
        pacotes_video: resumo_pacotes.pacotes_video,
        pacotes_audio: resumo_pacotes.pacotes_audio,
        inicia_em_keyframe: resumo_pacotes.primeiro_pacote_video_eh_keyframe,
    })
}

/// Valida que codecs, resolução, formato de pixel, proporção e faixas de áudio foram
/// preservados integralmente (sem recodificação).
fn comparar_parametros_stream(
    original: &MetadadosVideo,
    segmento: &MetadadosVideo,
    indice: usize,
) -> Resultado<()> {
    let video_orig =
        original
            .stream_video_principal()
            .ok_or_else(|| ErroDivisor::SemStreamVideo {
                caminho: original.caminho.clone(),
            })?;
    let video_seg =
        segmento
            .stream_video_principal()
            .ok_or_else(|| ErroDivisor::FalhaValidacao {
                detalhe: format!("Segmento {indice} perdeu o fluxo de vídeo principal."),
            })?;

    if video_orig.codec_nome != video_seg.codec_nome {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: format!(
                "Codec de vídeo alterado no segmento {indice}: esperado '{}', obtido '{}'.",
                video_orig.codec_nome, video_seg.codec_nome
            ),
        });
    }

    if video_orig.largura != video_seg.largura || video_orig.altura != video_seg.altura {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: format!(
                "Resolução alterada no segmento {indice}: esperado {}x{}, obtido {}x{}.",
                video_orig.largura, video_orig.altura, video_seg.largura, video_seg.altura
            ),
        });
    }

    if let (Some(pix_orig), Some(pix_seg)) = (&video_orig.formato_pixel, &video_seg.formato_pixel)
        && pix_orig != pix_seg
    {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: format!(
                "Formato de pixel alterado no segmento {indice}: esperado '{pix_orig}', obtido '{pix_seg}'."
            ),
        });
    }

    if let (Some(dar_orig), Some(dar_seg)) = (&video_orig.dar, &video_seg.dar)
        && dar_orig != dar_seg
    {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: format!(
                "Proporção (DAR) alterada no segmento {indice}: esperado '{dar_orig}', obtido '{dar_seg}'."
            ),
        });
    }

    // Preservação integral de todas as faixas de áudio (incluindo múltiplas faixas).
    if original.streams_audio.len() != segmento.streams_audio.len() {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: format!(
                "Quantidade de faixas de áudio divergente no segmento {indice}: esperado {}, obtido {}.",
                original.streams_audio.len(),
                segmento.streams_audio.len()
            ),
        });
    }

    for (faixa_idx, (a_orig, a_seg)) in original
        .streams_audio
        .iter()
        .zip(segmento.streams_audio.iter())
        .enumerate()
    {
        if a_orig.codec_nome != a_seg.codec_nome {
            return Err(ErroDivisor::FalhaValidacao {
                detalhe: format!(
                    "Codec de áudio da faixa {} alterado no segmento {indice}: esperado '{}', obtido '{}'.",
                    faixa_idx + 1,
                    a_orig.codec_nome,
                    a_seg.codec_nome
                ),
            });
        }
        if let (Some(c_orig), Some(c_seg)) = (a_orig.canais, a_seg.canais)
            && c_orig != c_seg
        {
            return Err(ErroDivisor::FalhaValidacao {
                detalhe: format!(
                    "Canais de áudio da faixa {} alterados no segmento {indice}: esperado {c_orig}, obtido {c_seg}.",
                    faixa_idx + 1
                ),
            });
        }
        if let (Some(sr_orig), Some(sr_seg)) = (a_orig.taxa_amostragem, a_seg.taxa_amostragem)
            && sr_orig != sr_seg
        {
            return Err(ErroDivisor::FalhaValidacao {
                detalhe: format!(
                    "Taxa de amostragem da faixa {} alterada no segmento {indice}: esperado {sr_orig} Hz, obtido {sr_seg} Hz.",
                    faixa_idx + 1
                ),
            });
        }
    }

    Ok(())
}

/// Valida o conjunto completo de segmentos: limites de tamanho, integridade total de pacotes
/// (sem perda nem duplicação) e continuidade temporal em relação ao vídeo original.
pub fn validar_conjunto_segmentos(
    mapa_original: &MapaVideo,
    segmentos: &[SegmentoValidado],
    limite_maximo_bytes: u64,
) -> Resultado<RelatorioValidacaoConjunto> {
    if segmentos.is_empty() {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: "Nenhum segmento foi produzido.".to_string(),
        });
    }

    let mut maior_segmento_bytes = 0u64;
    let mut duracao_total_segmentos = 0.0f64;
    let mut total_pacotes_video_segmentos = 0u64;
    let mut total_pacotes_audio_segmentos = 0u64;

    for seg in segmentos {
        if seg.tamanho_bytes > limite_maximo_bytes {
            return Err(ErroDivisor::SegmentoExcedeuLimite {
                indice: seg.indice,
                tamanho_bytes: seg.tamanho_bytes,
                limite_bytes: limite_maximo_bytes,
            });
        }
        if !seg.inicia_em_keyframe {
            return Err(ErroDivisor::FalhaValidacao {
                detalhe: format!("Segmento {} não inicia em keyframe.", seg.indice),
            });
        }

        maior_segmento_bytes = maior_segmento_bytes.max(seg.tamanho_bytes);
        duracao_total_segmentos += seg.duracao_segundos;
        total_pacotes_video_segmentos =
            total_pacotes_video_segmentos.saturating_add(seg.pacotes_video);
        total_pacotes_audio_segmentos =
            total_pacotes_audio_segmentos.saturating_add(seg.pacotes_audio);
    }

    // Garantia estrita de nenhum quadro de vídeo perdido nem duplicado.
    if total_pacotes_video_segmentos != mapa_original.total_pacotes_video {
        return Err(ErroDivisor::FalhaValidacao {
            detalhe: format!(
                "Divergência na contagem total de pacotes de vídeo: original possui {}, soma dos segmentos possui {}.",
                mapa_original.total_pacotes_video, total_pacotes_video_segmentos
            ),
        });
    }

    // Garantia de integridade dos pacotes de áudio (tolerando eventual 1 pacote de priming por faixa/fronteira).
    if mapa_original.total_pacotes_audio > 0 {
        let num_faixas = mapa_original.metadados.streams_audio.len().max(1) as u64;
        let tolerancia_audio = (segmentos.len() as u64).saturating_mul(num_faixas);
        let diferenca_audio =
            total_pacotes_audio_segmentos.abs_diff(mapa_original.total_pacotes_audio);

        if diferenca_audio > tolerancia_audio {
            return Err(ErroDivisor::FalhaValidacao {
                detalhe: format!(
                    "Divergência significativa nos pacotes de áudio: original possui {}, soma dos segmentos possui {} (diferença: {diferenca_audio}).",
                    mapa_original.total_pacotes_audio, total_pacotes_audio_segmentos
                ),
            });
        }
    }

    // Validação de continuidade temporal entre a soma dos segmentos e o vídeo original.
    let duracao_orig = mapa_original.metadados.duracao_segundos;
    if duracao_orig > 0.0 {
        // Permite pequena margem por priming de áudio/timestamps de borda (0.15s por segmento ou 1.5%).
        #[allow(clippy::cast_precision_loss)]
        let tolerancia_tempo = (segmentos.len() as f64 * 0.15)
            .max(duracao_orig * 0.015)
            .max(0.5);
        let diferenca_tempo = (duracao_total_segmentos - duracao_orig).abs();

        if diferenca_tempo > tolerancia_tempo {
            return Err(ErroDivisor::FalhaValidacao {
                detalhe: format!(
                    "Continuidade temporal violada: duração original {:.3}s vs soma dos segmentos {:.3}s (diferença {:.3}s > tolerância {:.3}s).",
                    duracao_orig, duracao_total_segmentos, diferenca_tempo, tolerancia_tempo
                ),
            });
        }
    }

    Ok(RelatorioValidacaoConjunto {
        total_segmentos: segmentos.len(),
        duracao_total_segmentos,
        total_pacotes_video_segmentos,
        total_pacotes_audio_segmentos,
        maior_segmento_bytes,
    })
}
