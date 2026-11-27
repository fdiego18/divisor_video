//! Análise de metadados, fluxos, keyframes e pacotes de vídeo via FFprobe em streaming.
//!
//! Os pacotes são processados incrementalmente durante a leitura do `stdout` do FFprobe
//! e agregados em blocos GOP (intervalos entre keyframes consecutivos), garantindo
//! consumo mínimo e previsível de memória RAM mesmo em arquivos de vários gigabytes.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use serde::Deserialize;
use tracing::{debug, trace};

use crate::erros::{ErroDivisor, Resultado};
use crate::executor_ffmpeg::ExecutorFfmpeg;

/// Quantidade máxima de pacotes de áudio mantidos na janela de alinhamento de interleaving.
const TAMANHO_JANELA_AUDIO: usize = 512;

/// Informações detalhadas de um fluxo de vídeo original.
#[derive(Debug, Clone)]
pub struct InfoStreamVideo {
    /// Índice original do stream no contêiner.
    pub indice: usize,
    /// Nome do codec (ex: `h264`, `hevc`, `vp9`, `mpeg4`).
    pub codec_nome: String,
    /// Tag de codec no contêiner, quando disponível.
    pub codec_tag: Option<String>,
    /// Largura do quadro em pixels.
    pub largura: u32,
    /// Altura do quadro em pixels.
    pub altura: u32,
    /// Formato de pixel (ex: `yuv420p`).
    pub formato_pixel: Option<String>,
    /// Taxa de quadros média em forma racional (ex: `"30/1"`).
    pub fps_racional: String,
    /// Taxa de quadros numérica aproximada.
    pub fps: f64,
    /// Bitrate informado do stream, se disponível.
    pub bitrate: Option<u64>,
    /// Razão de aspecto da amostra (SAR).
    pub sar: Option<String>,
    /// Razão de aspecto de exibição (DAR).
    pub dar: Option<String>,
}

/// Informações detalhadas de um fluxo de áudio original.
#[derive(Debug, Clone)]
pub struct InfoStreamAudio {
    /// Índice original do stream no contêiner.
    pub indice: usize,
    /// Nome do codec (ex: `aac`, `opus`, `mp3`).
    pub codec_nome: String,
    /// Quantidade de canais de áudio.
    pub canais: Option<u32>,
    /// Taxa de amostragem em Hz.
    pub taxa_amostragem: Option<u32>,
    /// Bitrate informado do stream, se disponível.
    pub bitrate: Option<u64>,
    /// Código de idioma nos metadados, quando presente.
    pub idioma: Option<String>,
}

/// Metadados estruturais completos de um arquivo de vídeo.
#[derive(Debug, Clone)]
pub struct MetadadosVideo {
    /// Caminho do arquivo original.
    pub caminho: PathBuf,
    /// Nome do formato/contêiner reportado pelo FFprobe.
    pub formato_nome: String,
    /// Tamanho real do arquivo em disco (em bytes).
    pub tamanho_bytes: u64,
    /// Duração total em segundos.
    pub duracao_segundos: f64,
    /// Timestamp inicial do formato (`start_time`), quando presente.
    pub tempo_inicial_formato: f64,
    /// Bitrate global em bits/s, quando informado.
    pub bitrate_geral: Option<u64>,
    /// Índice do stream de vídeo principal utilizado como referência de keyframes.
    pub indice_stream_video_principal: usize,
    /// Fluxos de vídeo válidos presentes no arquivo.
    pub streams_video: Vec<InfoStreamVideo>,
    /// Fluxos de áudio presentes no arquivo (suporta múltiplas faixas).
    pub streams_audio: Vec<InfoStreamAudio>,
    /// Lista ordenada de índices de streams que serão copiados integralmente (`-map`).
    pub indices_streams_mapeados: Vec<usize>,
}

impl MetadadosVideo {
    /// Retorna a descrição do fluxo de vídeo principal.
    pub fn stream_video_principal(&self) -> Option<&InfoStreamVideo> {
        self.streams_video
            .iter()
            .find(|s| s.indice == self.indice_stream_video_principal)
            .or_else(|| self.streams_video.first())
    }
}

/// Unidade indivisível de corte sem recodificação: todos os pacotes (vídeo + áudio)
/// compreendidos entre um keyframe e o keyframe imediatamente seguinte.
#[derive(Debug, Clone)]
pub struct BlocoGop {
    /// Índice sequencial do bloco (base 0).
    pub indice: usize,
    /// PTS (em segundos) do keyframe inicial deste bloco.
    pub pts_inicio: f64,
    /// DTS (em segundos) do keyframe inicial deste bloco.
    pub dts_inicio: f64,
    /// PTS (em segundos) do final deste bloco (início do próximo keyframe ou fim do vídeo).
    pub pts_fim: f64,
    /// Duração efetiva deste bloco em segundos.
    pub duracao_segundos: f64,
    /// Soma dos bytes dos pacotes de vídeo pertencentes a este bloco.
    pub bytes_video: u64,
    /// Soma dos bytes dos pacotes de áudio de todas as faixas neste bloco.
    pub bytes_audio: u64,
    /// Soma dos bytes de outros pacotes mapeados neste bloco.
    pub bytes_outros: u64,
    /// Soma total de bytes de payload (vídeo + áudio + outros) neste bloco.
    pub bytes_totais_pacotes: u64,
    /// Quantidade de pacotes do stream de vídeo principal neste bloco.
    pub quantidade_pacotes_video: u64,
    /// Quantidade de pacotes de áudio (todas as faixas) neste bloco.
    pub quantidade_pacotes_audio: u64,
}

impl BlocoGop {
    /// Quantidade total de pacotes contabilizados neste bloco.
    pub fn total_pacotes(&self) -> u64 {
        self.quantidade_pacotes_video
            .saturating_add(self.quantidade_pacotes_audio)
    }

    /// Bitrate aproximado deste bloco em bytes por segundo.
    pub fn taxa_bytes_por_segundo(&self) -> f64 {
        if self.duracao_segundos > 0.001 {
            #[allow(clippy::cast_precision_loss)]
            let taxa = self.bytes_totais_pacotes as f64 / self.duracao_segundos;
            taxa
        } else {
            0.0
        }
    }
}

/// Mapa estrutural completo do vídeo após análise de metadados e pacotes.
#[derive(Debug, Clone)]
pub struct MapaVideo {
    /// Metadados gerais e dos fluxos.
    pub metadados: MetadadosVideo,
    /// Blocos GOP em ordem cronológica, cada um iniciando exatamente em um keyframe.
    pub blocos_gop: Vec<BlocoGop>,
    /// Soma total de bytes de todos os pacotes mapeados no arquivo original.
    pub soma_bytes_pacotes: u64,
    /// Total de pacotes do stream de vídeo principal no arquivo original.
    pub total_pacotes_video: u64,
    /// Total de pacotes de áudio (todas as faixas) no arquivo original.
    pub total_pacotes_audio: u64,
    /// Sobrecarga (overhead) total medida no contêiner original em bytes.
    pub overhead_total_observado: u64,
    /// Overhead médio estimado por pacote (tabelas de índice/cabeçalhos).
    pub overhead_por_pacote_estimado: f64,
    /// Overhead fixo mínimo estimado por arquivo gerado.
    pub overhead_fixo_estimado: u64,
    /// Indica se o vídeo possui bitrate variável (VBR) significativo.
    pub eh_vbr: bool,
}

/// Estruturas de desserialização direta do JSON do FFprobe (`-show_format -show_streams`).
#[derive(Debug, Deserialize)]
struct SaidaMetadadosFfprobe {
    format: Option<FormatoFfprobe>,
    #[serde(default)]
    streams: Vec<StreamFfprobe>,
}

#[derive(Debug, Deserialize)]
struct FormatoFfprobe {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
    start_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamFfprobe {
    index: usize,
    codec_type: Option<String>,
    codec_name: Option<String>,
    codec_tag_string: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    bit_rate: Option<String>,
    sample_aspect_ratio: Option<String>,
    display_aspect_ratio: Option<String>,
    channels: Option<u32>,
    sample_rate: Option<String>,
    #[serde(default)]
    disposition: HashMap<String, i64>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

/// Estrutura de desserialização de cada pacote emitido por `ffprobe -show_packets -of json`.
#[derive(Debug, Deserialize)]
struct PacoteFfprobe {
    stream_index: usize,
    codec_type: Option<String>,
    pts_time: Option<String>,
    dts_time: Option<String>,
    duration_time: Option<String>,
    size: Option<String>,
    flags: Option<String>,
}

/// Analisa apenas os metadados estruturais (`format` e `streams`) de um arquivo.
pub fn analisar_metadados(executor: &ExecutorFfmpeg, caminho: &Path) -> Resultado<MetadadosVideo> {
    let tamanho_bytes = fs::metadata(caminho)
        .map_err(|erro| {
            ErroDivisor::io(
                format!("leitura de metadados de '{}'", caminho.display()),
                erro,
            )
        })?
        .len();

    if tamanho_bytes == 0 {
        return Err(ErroDivisor::ArquivoCorrompido {
            caminho: caminho.to_path_buf(),
            detalhe: "Arquivo está vazio (0 bytes).".to_string(),
        });
    }

    let json_bruto = executor.obter_metadados_json(caminho)?;
    interpretar_metadados_json(caminho, tamanho_bytes, &json_bruto)
}

/// Interpreta o JSON de metadados do FFprobe em `MetadadosVideo`.
pub fn interpretar_metadados_json(
    caminho: &Path,
    tamanho_bytes_disco: u64,
    json_bruto: &str,
) -> Resultado<MetadadosVideo> {
    let parsed: SaidaMetadadosFfprobe =
        serde_json::from_str(json_bruto).map_err(|erro| ErroDivisor::FalhaInterpretacaoJson {
            detalhe: erro.to_string(),
        })?;

    let formato = parsed
        .format
        .ok_or_else(|| ErroDivisor::ArquivoCorrompido {
            caminho: caminho.to_path_buf(),
            detalhe: "Objeto 'format' ausente na resposta do FFprobe.".to_string(),
        })?;

    let mut streams_video = Vec::new();
    let mut streams_audio = Vec::new();
    let mut indices_streams_mapeados = Vec::new();

    for stream in parsed.streams {
        let tipo = stream.codec_type.as_deref().unwrap_or("");
        let eh_capa_anexada = stream.disposition.get("attached_pic").copied().unwrap_or(0) == 1;

        match tipo {
            "video" if !eh_capa_anexada => {
                let fps_racional = stream
                    .avg_frame_rate
                    .clone()
                    .filter(|s| s != "0/0" && !s.is_empty())
                    .or_else(|| stream.r_frame_rate.clone())
                    .unwrap_or_else(|| "0/1".to_string());

                let fps = converter_racional_f64(&fps_racional).unwrap_or(0.0);

                streams_video.push(InfoStreamVideo {
                    indice: stream.index,
                    codec_nome: stream
                        .codec_name
                        .unwrap_or_else(|| "desconhecido".to_string()),
                    codec_tag: stream.codec_tag_string,
                    largura: stream.width.unwrap_or(0),
                    altura: stream.height.unwrap_or(0),
                    formato_pixel: stream.pix_fmt,
                    fps_racional,
                    fps,
                    bitrate: stream
                        .bit_rate
                        .as_deref()
                        .and_then(|s| s.parse::<u64>().ok()),
                    sar: stream.sample_aspect_ratio,
                    dar: stream.display_aspect_ratio,
                });
                indices_streams_mapeados.push(stream.index);
            }
            "audio" => {
                let idioma = stream
                    .tags
                    .get("language")
                    .or_else(|| stream.tags.get("LANGUAGE"))
                    .cloned();

                streams_audio.push(InfoStreamAudio {
                    indice: stream.index,
                    codec_nome: stream
                        .codec_name
                        .unwrap_or_else(|| "desconhecido".to_string()),
                    canais: stream.channels,
                    taxa_amostragem: stream
                        .sample_rate
                        .as_deref()
                        .and_then(|s| s.parse::<u32>().ok()),
                    bitrate: stream
                        .bit_rate
                        .as_deref()
                        .and_then(|s| s.parse::<u64>().ok()),
                    idioma,
                });
                indices_streams_mapeados.push(stream.index);
            }
            _ => {}
        }
    }

    let video_principal = streams_video
        .first()
        .ok_or_else(|| ErroDivisor::SemStreamVideo {
            caminho: caminho.to_path_buf(),
        })?;
    let indice_stream_video_principal = video_principal.indice;

    let duracao_segundos = formato
        .duration
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d >= 0.0)
        .unwrap_or(0.0);

    let tempo_inicial_formato = formato
        .start_time
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|t| t.is_finite())
        .unwrap_or(0.0);

    let tamanho_bytes = if tamanho_bytes_disco > 0 {
        tamanho_bytes_disco
    } else {
        formato
            .size
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    };

    let bitrate_geral = formato
        .bit_rate
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok());

    indices_streams_mapeados.sort_unstable();

    Ok(MetadadosVideo {
        caminho: caminho.to_path_buf(),
        formato_nome: formato
            .format_name
            .unwrap_or_else(|| "desconhecido".to_string()),
        tamanho_bytes,
        duracao_segundos,
        tempo_inicial_formato,
        bitrate_geral,
        indice_stream_video_principal,
        streams_video,
        streams_audio,
        indices_streams_mapeados,
    })
}

/// Realiza a análise completa do vídeo (metadados + leitura em streaming dos pacotes e keyframes).
pub fn analisar_video(
    executor: &ExecutorFfmpeg,
    caminho: &Path,
    cancelamento: &Arc<AtomicBool>,
) -> Resultado<MapaVideo> {
    if cancelamento.load(Ordering::Relaxed) {
        return Err(ErroDivisor::CanceladoPeloUsuario);
    }

    let metadados = analisar_metadados(executor, caminho)?;

    let mut filho = executor.iniciar_leitura_pacotes_json(caminho)?;
    let stdout = filho
        .stdout
        .take()
        .ok_or_else(|| ErroDivisor::FalhaExecucaoFfprobe {
            detalhe: "Não foi possível capturar stdout do ffprobe.".to_string(),
        })?;
    let stderr = filho
        .stderr
        .take()
        .ok_or_else(|| ErroDivisor::FalhaExecucaoFfprobe {
            detalhe: "Não foi possível capturar stderr do ffprobe.".to_string(),
        })?;

    let coletor_stderr = thread::spawn(move || {
        let mut texto = String::new();
        let mut leitor = BufReader::new(stderr);
        let _ = leitor.read_to_string(&mut texto);
        texto
    });

    let leitor_streaming = LeitorStreamingPacotesJson::novo(BufReader::new(stdout));
    let resultado_agregacao =
        agregar_pacotes_em_blocos(leitor_streaming, &metadados, cancelamento, &mut filho);

    let status = filho
        .wait()
        .map_err(|erro| ErroDivisor::io("aguardando encerramento do ffprobe", erro))?;
    let texto_stderr = coletor_stderr.join().unwrap_or_default();

    if cancelamento.load(Ordering::Relaxed) {
        return Err(ErroDivisor::CanceladoPeloUsuario);
    }

    let mapa = resultado_agregacao?;

    if !status.success() {
        return Err(ErroDivisor::ArquivoCorrompido {
            caminho: caminho.to_path_buf(),
            detalhe: if texto_stderr.trim().is_empty() {
                format!("ffprobe falhou ao ler pacotes (código {:?})", status.code())
            } else {
                texto_stderr.trim().to_string()
            },
        });
    }

    debug!(
        arquivo = %caminho.display(),
        blocos_gop = mapa.blocos_gop.len(),
        pacotes_video = mapa.total_pacotes_video,
        pacotes_audio = mapa.total_pacotes_audio,
        overhead_total = mapa.overhead_total_observado,
        eh_vbr = mapa.eh_vbr,
        "Análise completa do vídeo concluída"
    );

    Ok(mapa)
}

/// Pacote de áudio pendente na janela de alinhamento cronológico com keyframes.
#[derive(Debug, Clone, Copy)]
struct PacoteAudioPendente {
    tempo: f64,
    bytes: u64,
}

/// Agrega os pacotes vindos do leitor streaming JSON em blocos GOP com memória limitada.
fn agregar_pacotes_em_blocos<R: BufRead>(
    mut leitor: LeitorStreamingPacotesJson<R>,
    metadados: &MetadadosVideo,
    cancelamento: &Arc<AtomicBool>,
    filho: &mut std::process::Child,
) -> Resultado<MapaVideo> {
    let streams_mapeados: HashSet<usize> =
        metadados.indices_streams_mapeados.iter().copied().collect();
    let idx_video_principal = metadados.indice_stream_video_principal;

    let mut blocos: Vec<BlocoGop> = Vec::new();
    let mut janela_audio: VecDeque<PacoteAudioPendente> =
        VecDeque::with_capacity(TAMANHO_JANELA_AUDIO);

    let mut total_pacotes_video = 0u64;
    let mut total_pacotes_audio = 0u64;
    let mut soma_bytes_pacotes = 0u64;
    let mut maior_tempo_video = 0.0f64;
    let mut tem_tempo_video = false;
    let mut contador_verificacao_cancelamento = 0u64;

    while let Some(pacote) = leitor.proximo_pacote()? {
        contador_verificacao_cancelamento = contador_verificacao_cancelamento.wrapping_add(1);
        if contador_verificacao_cancelamento.is_multiple_of(4096)
            && cancelamento.load(Ordering::Relaxed)
        {
            let _ = filho.kill();
            return Err(ErroDivisor::CanceladoPeloUsuario);
        }

        if !streams_mapeados.contains(&pacote.stream_index) {
            continue;
        }

        let tamanho = pacote
            .size
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let pts = pacote
            .pts_time
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite());
        let dts = pacote
            .dts_time
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite());
        let duracao_pkt = pacote
            .duration_time
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(0.0);
        let flags = pacote.flags.as_deref().unwrap_or("");
        let codec_type = pacote.codec_type.as_deref().unwrap_or("");

        let tempo_ref = pts.or(dts).unwrap_or(0.0);
        soma_bytes_pacotes = soma_bytes_pacotes.saturating_add(tamanho);

        if pacote.stream_index == idx_video_principal {
            total_pacotes_video = total_pacotes_video.saturating_add(1);
            let fim_pkt = tempo_ref + duracao_pkt;
            if !tem_tempo_video || fim_pkt > maior_tempo_video {
                maior_tempo_video = fim_pkt;
                tem_tempo_video = true;
            }

            let eh_keyframe = flags.contains('K');
            if eh_keyframe || blocos.is_empty() {
                let novo_indice = blocos.len();
                let pts_kf = pts.or(dts).unwrap_or(0.0);
                let dts_kf = dts.or(pts).unwrap_or(pts_kf);

                blocos.push(BlocoGop {
                    indice: novo_indice,
                    pts_inicio: pts_kf,
                    dts_inicio: dts_kf,
                    pts_fim: pts_kf,
                    duracao_segundos: 0.0,
                    bytes_video: tamanho,
                    bytes_audio: 0,
                    bytes_outros: 0,
                    bytes_totais_pacotes: tamanho,
                    quantidade_pacotes_video: 1,
                    quantidade_pacotes_audio: 0,
                });

                // Assim que um novo keyframe entra, drenamos pacotes de áudio da janela
                // que definitivamente pertencem a blocos anteriores.
                while janela_audio.len() >= TAMANHO_JANELA_AUDIO / 2 {
                    if let Some(pendente) = janela_audio.pop_front() {
                        atribuir_pacote_audio(&mut blocos, pendente);
                    }
                }
            } else if let Some(ultimo) = blocos.last_mut() {
                ultimo.bytes_video = ultimo.bytes_video.saturating_add(tamanho);
                ultimo.bytes_totais_pacotes = ultimo.bytes_totais_pacotes.saturating_add(tamanho);
                ultimo.quantidade_pacotes_video = ultimo.quantidade_pacotes_video.saturating_add(1);
            }
        } else if codec_type == "audio" {
            total_pacotes_audio = total_pacotes_audio.saturating_add(1);
            janela_audio.push_back(PacoteAudioPendente {
                tempo: tempo_ref,
                bytes: tamanho,
            });
            if janela_audio.len() > TAMANHO_JANELA_AUDIO
                && let Some(pendente) = janela_audio.pop_front()
            {
                if !blocos.is_empty() {
                    atribuir_pacote_audio(&mut blocos, pendente);
                } else {
                    // Caso raro: muitos pacotes de áudio antes do 1º pacote de vídeo;
                    // mantemos no início até o 1º keyframe surgir ou acumulamos ao final.
                    janela_audio.push_front(pendente);
                    if janela_audio.len() > TAMANHO_JANELA_AUDIO * 4 {
                        // Agrega preventivamente em um único item consolidado para manter RAM O(1).
                        let mut bytes_acumulados = 0u64;
                        let mut tempo_min = f64::INFINITY;
                        while let Some(item) = janela_audio.pop_front() {
                            bytes_acumulados = bytes_acumulados.saturating_add(item.bytes);
                            tempo_min = tempo_min.min(item.tempo);
                        }
                        janela_audio.push_back(PacoteAudioPendente {
                            tempo: if tempo_min.is_finite() {
                                tempo_min
                            } else {
                                0.0
                            },
                            bytes: bytes_acumulados,
                        });
                    }
                }
            }
        } else if let Some(ultimo) = blocos.last_mut() {
            ultimo.bytes_outros = ultimo.bytes_outros.saturating_add(tamanho);
            ultimo.bytes_totais_pacotes = ultimo.bytes_totais_pacotes.saturating_add(tamanho);
        }
    }

    if blocos.is_empty() || total_pacotes_video == 0 {
        return Err(ErroDivisor::SemKeyframes {
            caminho: metadados.caminho.clone(),
            detalhe: "Nenhum pacote de vídeo/keyframe legível encontrado pelo FFprobe.".to_string(),
        });
    }

    // Drena todos os pacotes de áudio restantes na janela atribuindo ao bloco GOP exato.
    while let Some(pendente) = janela_audio.pop_front() {
        atribuir_pacote_audio(&mut blocos, pendente);
    }

    // Calcula limites temporais e duração exata de cada bloco GOP.
    let fim_total = if tem_tempo_video {
        maior_tempo_video.max(metadados.tempo_inicial_formato + metadados.duracao_segundos)
    } else {
        metadados.tempo_inicial_formato + metadados.duracao_segundos
    };

    for i in 0..blocos.len() {
        let fim = if i + 1 < blocos.len() {
            blocos[i + 1].pts_inicio.max(blocos[i].pts_inicio)
        } else {
            fim_total.max(blocos[i].pts_inicio)
        };
        blocos[i].pts_fim = fim;
        blocos[i].duracao_segundos = (fim - blocos[i].pts_inicio).max(0.0);
    }

    let overhead_total_observado = metadados.tamanho_bytes.saturating_sub(soma_bytes_pacotes);
    let total_pacotes = total_pacotes_video
        .saturating_add(total_pacotes_audio)
        .max(1);

    // Estimativa inicial conservadora do overhead do contêiner:
    // - Parte proporcional aos pacotes (índices stsz/stco/cues/idx1)
    // - Parte fixa por arquivo de saída (ftyp/moov base ou header EBML/RIFF)
    #[allow(clippy::cast_precision_loss)]
    let overhead_por_pacote_estimado =
        (overhead_total_observado as f64 / total_pacotes as f64).clamp(8.0, 256.0);
    let overhead_fixo_estimado = estimar_overhead_fixo_inicial(&metadados.formato_nome);
    let eh_vbr = detectar_vbr(&blocos);

    trace!(
        overhead_por_pacote = overhead_por_pacote_estimado,
        overhead_fixo = overhead_fixo_estimado,
        eh_vbr,
        "Parâmetros de overhead e variação de bitrate estimados"
    );

    Ok(MapaVideo {
        metadados: metadados.clone(),
        blocos_gop: blocos,
        soma_bytes_pacotes,
        total_pacotes_video,
        total_pacotes_audio,
        overhead_total_observado,
        overhead_por_pacote_estimado,
        overhead_fixo_estimado,
        eh_vbr,
    })
}

/// Atribui um pacote de áudio ao bloco GOP cujo intervalo temporal o contém usando busca binária.
fn atribuir_pacote_audio(blocos: &mut [BlocoGop], pacote: PacoteAudioPendente) {
    if blocos.is_empty() {
        return;
    }

    // Encontra o primeiro bloco cujo pts_inicio é maior que o tempo do pacote.
    let pos = blocos.partition_point(|b| b.pts_inicio <= pacote.tempo);
    let indice_destino = pos.saturating_sub(1);
    let bloco = &mut blocos[indice_destino];
    bloco.bytes_audio = bloco.bytes_audio.saturating_add(pacote.bytes);
    bloco.bytes_totais_pacotes = bloco.bytes_totais_pacotes.saturating_add(pacote.bytes);
    bloco.quantidade_pacotes_audio = bloco.quantidade_pacotes_audio.saturating_add(1);
}

/// Estima um overhead fixo inicial razoável conforme a família do contêiner.
fn estimar_overhead_fixo_inicial(formato_nome: &str) -> u64 {
    let lower = formato_nome.to_ascii_lowercase();
    if lower.contains("mp4") || lower.contains("mov") || lower.contains("3gp") {
        // Cabeçalho ftyp + estrutura moov/trak/stbl base
        24_576
    } else if lower.contains("matroska") || lower.contains("webm") {
        // Cabeçalho EBML + Segment Info + Tracks + Cues base
        16_384
    } else if lower.contains("avi") {
        // Cabeçalho RIFF + hdrl + idx1 base
        32_768
    } else {
        24_576
    }
}

/// Detecta se o vídeo apresenta variação significativa de bitrate (VBR) entre seus blocos GOP.
pub fn detectar_vbr(blocos: &[BlocoGop]) -> bool {
    let taxas: Vec<f64> = blocos
        .iter()
        .filter(|b| b.duracao_segundos >= 0.25)
        .map(BlocoGop::taxa_bytes_por_segundo)
        .filter(|t| *t > 0.0)
        .collect();

    if taxas.len() < 3 {
        return false;
    }

    #[allow(clippy::cast_precision_loss)]
    let media = taxas.iter().sum::<f64>() / taxas.len() as f64;
    if media <= 0.0 {
        return false;
    }

    #[allow(clippy::cast_precision_loss)]
    let variancia = taxas
        .iter()
        .map(|t| {
            let diff = t - media;
            diff * diff
        })
        .sum::<f64>()
        / taxas.len() as f64;

    let coef_variacao = variancia.sqrt() / media;
    // Coeficiente de variação acima de 18% caracteriza claramente VBR.
    coef_variacao > 0.18
}

/// Converte uma fração racional do FFprobe (ex: `"30000/1001"`) em `f64`.
pub fn converter_racional_f64(racional: &str) -> Option<f64> {
    let partes: Vec<&str> = racional.trim().split('/').collect();
    match partes.as_slice() {
        [num] => num.parse::<f64>().ok().filter(|v| v.is_finite()),
        [num, den] => {
            let n = num.parse::<f64>().ok()?;
            let d = den.parse::<f64>().ok()?;
            if d.abs() < f64::EPSILON {
                None
            } else {
                let res = n / d;
                res.is_finite().then_some(res)
            }
        }
        _ => None,
    }
}

/// Leitor streaming incremental de objetos JSON do array `"packets": [...]` gerado pelo FFprobe.
///
/// Mantém apenas um único objeto de pacote na memória por vez, respeitando objetos aninhados
/// (como `side_data_list`), strings e sequências de escape JSON.
struct LeitorStreamingPacotesJson<R: BufRead> {
    leitor: R,
    dentro_array_pacotes: bool,
    finalizado: bool,
    buffer_objeto: String,
    buffer_linha: String,
    posicao_linha: usize,
}

impl<R: BufRead> LeitorStreamingPacotesJson<R> {
    fn novo(leitor: R) -> Self {
        Self {
            leitor,
            dentro_array_pacotes: false,
            finalizado: false,
            buffer_objeto: String::with_capacity(512),
            buffer_linha: String::with_capacity(256),
            posicao_linha: 0,
        }
    }

    fn proximo_byte(&mut self) -> Resultado<Option<u8>> {
        loop {
            if self.posicao_linha < self.buffer_linha.len() {
                let b = self.buffer_linha.as_bytes()[self.posicao_linha];
                self.posicao_linha += 1;
                return Ok(Some(b));
            }
            self.buffer_linha.clear();
            self.posicao_linha = 0;
            let lidos = self
                .leitor
                .read_line(&mut self.buffer_linha)
                .map_err(|erro| ErroDivisor::io("leitura em streaming do JSON do ffprobe", erro))?;
            if lidos == 0 {
                return Ok(None);
            }
        }
    }

    fn avancar_ate_array_pacotes(&mut self) -> Resultado<bool> {
        if self.dentro_array_pacotes {
            return Ok(true);
        }
        // Procura o primeiro '[' que abre o array "packets".
        while let Some(b) = self.proximo_byte()? {
            if b == b'[' {
                self.dentro_array_pacotes = true;
                return Ok(true);
            }
        }
        self.finalizado = true;
        Ok(false)
    }

    fn proximo_pacote(&mut self) -> Resultado<Option<PacoteFfprobe>> {
        if self.finalizado {
            return Ok(None);
        }

        if !self.avancar_ate_array_pacotes()? {
            return Ok(None);
        }

        self.buffer_objeto.clear();
        let mut profundidade = 0usize;
        let mut dentro_string = false;
        let mut escapado = false;
        let mut iniciou_objeto = false;

        while let Some(b) = self.proximo_byte()? {
            if !iniciou_objeto {
                match b {
                    b'{' => {
                        iniciou_objeto = true;
                        profundidade = 1;
                        self.buffer_objeto.push('{');
                    }
                    b']' => {
                        self.finalizado = true;
                        return Ok(None);
                    }
                    _ => continue,
                }
                continue;
            }

            self.buffer_objeto.push(char::from(b));

            if dentro_string {
                if escapado {
                    escapado = false;
                } else if b == b'\\' {
                    escapado = true;
                } else if b == b'"' {
                    dentro_string = false;
                }
            } else {
                match b {
                    b'"' => dentro_string = true,
                    b'{' => profundidade += 1,
                    b'}' => {
                        profundidade = profundidade.saturating_sub(1);
                        if profundidade == 0 {
                            let pacote: PacoteFfprobe = serde_json::from_str(&self.buffer_objeto)
                                .map_err(|erro| {
                                ErroDivisor::FalhaInterpretacaoJson {
                                    detalhe: format!("Erro ao desserializar pacote JSON: {erro}"),
                                }
                            })?;
                            return Ok(Some(pacote));
                        }
                    }
                    _ => {}
                }
            }
        }

        self.finalizado = true;
        Ok(None)
    }
}

#[cfg(test)]
mod testes {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn leitor_streaming_json_interpreta_pacotes_com_side_data_aninhado() {
        let json = r#"{
            "packets": [
                {
                    "codec_type": "video",
                    "stream_index": 0,
                    "pts_time": "0.000000",
                    "dts_time": "-0.040000",
                    "duration_time": "0.040000",
                    "size": "4500",
                    "flags": "K__"
                },
                {
                    "codec_type": "audio",
                    "stream_index": 1,
                    "pts_time": "0.000000",
                    "dts_time": "0.000000",
                    "duration_time": "0.023220",
                    "size": "256",
                    "flags": "K__",
                    "side_data_list": [
                        {
                            "side_data_type": "Skip \"Samples\" {nested}"
                        }
                    ]
                }
            ]
        }"#;

        let mut leitor = LeitorStreamingPacotesJson::novo(Cursor::new(json.as_bytes()));
        let p1 = leitor.proximo_pacote().ok().flatten();
        let p2 = leitor.proximo_pacote().ok().flatten();
        let p3 = leitor.proximo_pacote().ok().flatten();

        assert!(p1.is_some());
        assert!(p2.is_some());
        assert!(p3.is_none());

        if let Some(pkt) = p1 {
            assert_eq!(pkt.stream_index, 0);
            assert_eq!(pkt.size.as_deref(), Some("4500"));
            assert_eq!(pkt.flags.as_deref(), Some("K__"));
        }
        if let Some(pkt) = p2 {
            assert_eq!(pkt.stream_index, 1);
            assert_eq!(pkt.size.as_deref(), Some("256"));
        }
    }

    #[test]
    fn converte_taxas_racionais() {
        assert_eq!(converter_racional_f64("30/1"), Some(30.0));
        let ntsc = converter_racional_f64("30000/1001").unwrap_or(0.0);
        assert!((ntsc - 29.970_029_97).abs() < 0.001);
        assert_eq!(converter_racional_f64("0/0"), None);
    }

    #[test]
    fn detecta_vbr_e_cbr_corretamente() {
        let criar_bloco = |idx: usize, bytes: u64| BlocoGop {
            indice: idx,
            pts_inicio: idx as f64 * 2.0,
            dts_inicio: idx as f64 * 2.0,
            pts_fim: (idx + 1) as f64 * 2.0,
            duracao_segundos: 2.0,
            bytes_video: bytes,
            bytes_audio: 0,
            bytes_outros: 0,
            bytes_totais_pacotes: bytes,
            quantidade_pacotes_video: 60,
            quantidade_pacotes_audio: 0,
        };

        let cbr = vec![
            criar_bloco(0, 500_000),
            criar_bloco(1, 505_000),
            criar_bloco(2, 498_000),
            criar_bloco(3, 502_000),
        ];
        assert!(!detectar_vbr(&cbr));

        let vbr = vec![
            criar_bloco(0, 200_000),
            criar_bloco(1, 1_500_000),
            criar_bloco(2, 300_000),
            criar_bloco(3, 1_800_000),
        ];
        assert!(detectar_vbr(&vbr));
    }
}
