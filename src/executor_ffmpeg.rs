//! Camada de detecção e execução segura de processos FFmpeg e FFprobe.
//!
//! Todas as invocações utilizam diretamente `std::process::Command` sem intermediários
//! de shell, garantindo portabilidade entre Linux, Windows e macOS e proteção contra
//! injeção de argumentos.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use tracing::{debug, trace};

use crate::erros::{ErroDivisor, Resultado};
use crate::integracao_so::OrigemFerramenta;
#[cfg(not(feature = "windows-bundle"))]
use crate::integracao_so::listar_candidatos_ferramenta;

/// Diagnóstico detalhado de um executável multimídia (`ffmpeg` ou `ffprobe`).
#[derive(Debug, Clone)]
pub struct InfoFerramenta {
    /// Nome da ferramenta (`"ffmpeg"` ou `"ffprobe"`).
    pub nome: String,
    /// Indica se o executável foi localizado e respondeu corretamente a `-version`.
    pub disponivel: bool,
    /// Caminho efetivamente utilizado, quando disponível.
    pub caminho: Option<PathBuf>,
    /// Origem onde a ferramenta foi localizada.
    pub origem: Option<OrigemFerramenta>,
    /// Primeira linha da versão retornada pelo executável.
    pub versao: Option<String>,
    /// Mensagem de diagnóstico em caso de indisponibilidade.
    pub erro: Option<String>,
}

/// Diagnóstico conjunto das dependências multimídia `ffmpeg` e `ffprobe`.
#[derive(Debug, Clone)]
pub struct DiagnosticoFerramentas {
    /// Estado do executável `ffmpeg`.
    pub ffmpeg: InfoFerramenta,
    /// Estado do executável `ffprobe`.
    pub ffprobe: InfoFerramenta,
}

impl DiagnosticoFerramentas {
    /// Retorna `true` somente se ambos `ffmpeg` e `ffprobe` estiverem operacionais.
    pub fn tudo_disponivel(&self) -> bool {
        self.ffmpeg.disponivel && self.ffprobe.disponivel
    }
}

/// Parâmetros para execução de segmentação sem perda via muxer `segment` do FFmpeg.
#[derive(Debug, Clone)]
pub struct ParametrosSegmentacao<'a> {
    /// Caminho do vídeo original (somente leitura).
    pub arquivo_entrada: &'a Path,
    /// Padrão de saída com especificador numérico (ex: `/tmp/.../video_parte_%03d.mp4`).
    pub padrao_saida: &'a Path,
    /// Extensão do contêiner de saída em minúsculas (ex: `"mp4"`, `"mkv"`).
    pub extensao_saida: &'a str,
    /// Índices exatos dos streams a mapear (todos de vídeo e todas as faixas de áudio).
    pub indices_streams: &'a [usize],
    /// Instantes relativos (em segundos) onde iniciar novos segmentos (alinhados aos keyframes).
    pub tempos_corte_segundos: &'a [f64],
}

/// Resumo leve dos pacotes de um segmento gerado, obtido em streaming pelo FFprobe.
#[derive(Debug, Clone)]
pub struct ResumoPacotesSegmento {
    /// Quantidade total de pacotes do stream de vídeo principal.
    pub pacotes_video: u64,
    /// Quantidade total de pacotes de todos os streams de áudio.
    pub pacotes_audio: u64,
    /// Indica se o primeiro pacote de vídeo do segmento possui flag de keyframe (`K`).
    pub primeiro_pacote_video_eh_keyframe: bool,
    /// Menor PTS observado no stream de vídeo principal.
    pub primeiro_pts_video: Option<f64>,
    /// Maior PTS observado no stream de vídeo principal.
    pub ultimo_pts_video: Option<f64>,
}

/// Executor encapsulado para invocações seguras de `ffmpeg` e `ffprobe`.
#[derive(Debug, Clone)]
pub struct ExecutorFfmpeg {
    caminho_ffmpeg: PathBuf,
    caminho_ffprobe: PathBuf,
    #[cfg(all(windows, feature = "windows-bundle"))]
    _ferramentas_protegidas: std::sync::Arc<Vec<std::fs::File>>,
}

impl ExecutorFfmpeg {
    /// Detecta automaticamente `ffmpeg` e `ffprobe` seguindo a ordem de prioridade:
    /// 1. Caminho personalizado informado.
    /// 2. Diretório do executável do aplicativo.
    /// 3. Subdiretório de ferramentas do aplicativo (`ferramentas/`, `tools/`, `bin/`).
    /// 4. `PATH` do sistema operacional.
    pub fn detectar(
        caminho_ffmpeg: Option<PathBuf>,
        caminho_ffprobe: Option<PathBuf>,
    ) -> Resultado<Self> {
        let diag = Self::diagnosticar(caminho_ffmpeg.as_deref(), caminho_ffprobe.as_deref());

        let ffmpeg = diag
            .ffmpeg
            .caminho
            .filter(|_| diag.ffmpeg.disponivel)
            .ok_or_else(|| ErroDivisor::FerramentaNaoEncontrada {
                ferramenta: diag.ffmpeg.erro.unwrap_or_else(|| "ffmpeg".to_string()),
            })?;

        let ffprobe = diag
            .ffprobe
            .caminho
            .filter(|_| diag.ffprobe.disponivel)
            .ok_or_else(|| ErroDivisor::FerramentaNaoEncontrada {
                ferramenta: diag.ffprobe.erro.unwrap_or_else(|| "ffprobe".to_string()),
            })?;

        debug!(
            ffmpeg = %ffmpeg.display(),
            ffprobe = %ffprobe.display(),
            "Executáveis FFmpeg e FFprobe detectados com sucesso"
        );

        #[cfg(all(windows, feature = "windows-bundle"))]
        let guards = {
            let mut guards = crate::ferramentas_distribuidas::proteger_execucao(&ffmpeg)
                .map_err(|e| ErroDivisor::io("proteção do FFmpeg distribuído", e))?;
            guards.extend(
                crate::ferramentas_distribuidas::proteger_execucao(&ffprobe)
                    .map_err(|e| ErroDivisor::io("proteção do FFprobe distribuído", e))?,
            );
            std::sync::Arc::new(guards)
        };
        Ok(Self {
            #[cfg(all(windows, feature = "windows-bundle"))]
            _ferramentas_protegidas: guards,
            caminho_ffmpeg: ffmpeg,
            caminho_ffprobe: ffprobe,
        })
    }

    /// Executa diagnóstico detalhado de disponibilidade, origem e versão de `ffmpeg` e `ffprobe`.
    pub fn diagnosticar(
        caminho_ffmpeg: Option<&Path>,
        caminho_ffprobe: Option<&Path>,
    ) -> DiagnosticoFerramentas {
        let ffmpeg = diagnosticar_ferramenta_individual("ffmpeg", caminho_ffmpeg);
        let ffprobe = diagnosticar_ferramenta_individual("ffprobe", caminho_ffprobe);
        DiagnosticoFerramentas { ffmpeg, ffprobe }
    }

    /// Retorna o caminho resolvido do executável `ffmpeg`.
    pub fn caminho_ffmpeg(&self) -> &Path {
        &self.caminho_ffmpeg
    }

    /// Retorna o caminho resolvido do executável `ffprobe`.
    pub fn caminho_ffprobe(&self) -> &Path {
        &self.caminho_ffprobe
    }

    /// Obtém os metadados completos (`format` e `streams`) de um arquivo em formato JSON via FFprobe.
    pub fn obter_metadados_json(&self, arquivo: &Path) -> Resultado<String> {
        let saida = criar_comando(&self.caminho_ffprobe)
            .args([
                "-v",
                "error",
                "-show_format",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(arquivo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|erro| ErroDivisor::io("invocação do ffprobe para metadados", erro))?;

        if !saida.status.success() {
            let stderr = String::from_utf8_lossy(&saida.stderr).trim().to_string();
            return Err(ErroDivisor::ArquivoCorrompido {
                caminho: arquivo.to_path_buf(),
                detalhe: if stderr.is_empty() {
                    format!("ffprobe encerrou com status {:?}", saida.status.code())
                } else {
                    stderr
                },
            });
        }

        String::from_utf8(saida.stdout).map_err(|erro| ErroDivisor::FalhaExecucaoFfprobe {
            detalhe: format!("Saída do ffprobe não é UTF-8 válida: {erro}"),
        })
    }

    /// Inicia um processo `ffprobe` emitindo pacotes em formato JSON no `stdout` para leitura em streaming.
    ///
    /// Esta abordagem evita carregar todos os pacotes do vídeo na memória RAM.
    pub fn iniciar_leitura_pacotes_json(&self, arquivo: &Path) -> Resultado<Child> {
        let filho = criar_comando(&self.caminho_ffprobe)
            .args([
                "-v",
                "error",
                "-show_packets",
                "-show_entries",
                "packet=stream_index,codec_type,pts_time,dts_time,duration_time,size,flags",
                "-of",
                "json",
            ])
            .arg(arquivo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|erro| ErroDivisor::io("inicialização do ffprobe para pacotes", erro))?;

        Ok(filho)
    }

    /// Executa a divisão do vídeo usando Stream Copy (`-c copy`) e o muxer `segment`.
    ///
    /// - Preserva 100% da qualidade original (zero recodificação).
    /// - Mapeia explicitamente todos os streams de vídeo e áudio selecionados.
    /// - Notifica progresso em segundos processados via `ao_progresso`.
    /// - Suporta cancelamento imediato e seguro via `cancelamento`.
    pub fn executar_segmentacao<F>(
        &self,
        params: &ParametrosSegmentacao<'_>,
        cancelamento: &Arc<AtomicBool>,
        mut ao_progresso: F,
    ) -> Resultado<()>
    where
        F: FnMut(f64),
    {
        if cancelamento.load(Ordering::Relaxed) {
            return Err(ErroDivisor::CanceladoPeloUsuario);
        }

        let entrada_absoluta = std::fs::canonicalize(params.arquivo_entrada)
            .unwrap_or_else(|_| params.arquivo_entrada.to_path_buf());

        let mut cmd = criar_comando(&self.caminho_ffmpeg);
        cmd.args([
            "-hide_banner",
            "-nostdin",
            "-y",
            "-v",
            "error",
            "-progress",
            "pipe:1",
            "-i",
        ]);
        cmd.arg(&entrada_absoluta);

        // Mapeamento explícito de todos os streams de vídeo e todas as faixas de áudio.
        if params.indices_streams.is_empty() {
            cmd.args(["-map", "0:v", "-map", "0:a?"]);
        } else {
            for indice in params.indices_streams {
                cmd.args(["-map", &format!("0:{indice}")]);
            }
        }

        // Preserva metadados globais do arquivo original (ex: câmera, data de criação, rotação).
        cmd.args(["-map_metadata", "0"]);

        // Stream copy puro: nunca recodifica vídeo nem áudio.
        cmd.args(["-c", "copy"]);

        // Configuração do muxer de segmentação exata em keyframes.
        cmd.args(["-f", "segment"]);

        if !params.tempos_corte_segundos.is_empty() {
            let lista_tempos = params
                .tempos_corte_segundos
                .iter()
                .map(|t| format!("{t:.6}"))
                .collect::<Vec<_>>()
                .join(",");
            cmd.args(["-segment_times", &lista_tempos]);
        }

        cmd.args(["-reset_timestamps", "1", "-segment_start_number", "1"]);

        // Para contêineres ISO BMFF (MP4/MOV/M4V), habilita faststart para reprodução web imediata.
        if matches!(params.extensao_saida, "mp4" | "mov" | "m4v" | "3gp") {
            cmd.args(["-segment_format_options", "movflags=+faststart"]);
        }

        // Executa no diretório pai do padrão para evitar que caracteres '%' no caminho
        // do diretório sejam interpretados como especificadores printf pelo muxer `segment`.
        if let (Some(dir_pai), Some(nome_padrao)) = (
            params.padrao_saida.parent(),
            params.padrao_saida.file_name(),
        ) && !dir_pai.as_os_str().is_empty()
        {
            cmd.current_dir(dir_pai);
            cmd.arg(nome_padrao);
        } else {
            cmd.arg(params.padrao_saida);
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        debug!(
            entrada = %params.arquivo_entrada.display(),
            padrao = %params.padrao_saida.display(),
            cortes = params.tempos_corte_segundos.len(),
            "Iniciando segmentação FFmpeg (-c copy)"
        );

        let mut filho = cmd
            .spawn()
            .map_err(|erro| ErroDivisor::io("inicialização do ffmpeg", erro))?;

        let stdout = filho
            .stdout
            .take()
            .ok_or_else(|| ErroDivisor::FalhaExecucaoFfmpeg {
                codigo: None,
                stderr: "Não foi possível capturar stdout de progresso do FFmpeg.".to_string(),
            })?;

        let stderr = filho
            .stderr
            .take()
            .ok_or_else(|| ErroDivisor::FalhaExecucaoFfmpeg {
                codigo: None,
                stderr: "Não foi possível capturar stderr do FFmpeg.".to_string(),
            })?;

        // Drena stderr em thread paralela para evitar deadlock em pipes quando há mensagens longas.
        let coletor_stderr = thread::spawn(move || {
            let mut texto = String::new();
            let mut leitor = BufReader::new(stderr);
            let _ = leitor.read_to_string(&mut texto);
            texto
        });

        let leitor_progresso = BufReader::new(stdout);
        let mut cancelado = false;

        for linha_res in leitor_progresso.lines() {
            if cancelamento.load(Ordering::Relaxed) {
                cancelado = true;
                let _ = filho.kill();
                break;
            }

            let linha = match linha_res {
                Ok(l) => l,
                Err(_) => break,
            };

            if let Some(valor_us) = linha.strip_prefix("out_time_us=")
                && let Ok(microssegundos) = valor_us.trim().parse::<i64>()
                && microssegundos >= 0
            {
                #[allow(clippy::cast_precision_loss)]
                let segundos = microssegundos as f64 / 1_000_000.0;
                trace!(segundos, "Progresso reportado pelo FFmpeg");
                ao_progresso(segundos);
            }
        }

        let status = filho
            .wait()
            .map_err(|erro| ErroDivisor::io("espera pela finalização do ffmpeg", erro))?;

        let texto_stderr = coletor_stderr.join().unwrap_or_default();

        if cancelado || cancelamento.load(Ordering::Relaxed) {
            return Err(ErroDivisor::CanceladoPeloUsuario);
        }

        if !status.success() {
            return Err(ErroDivisor::FalhaExecucaoFfmpeg {
                codigo: status.code(),
                stderr: texto_stderr.trim().to_string(),
            });
        }

        Ok(())
    }

    /// Inspeciona os pacotes de um segmento gerado em streaming (baixo consumo de RAM)
    /// para validar integridade, contagem de pacotes de vídeo/áudio e presença de keyframe inicial.
    pub fn inspecionar_pacotes_segmento(
        &self,
        arquivo: &Path,
        indice_stream_video_esperado: Option<usize>,
    ) -> Resultado<ResumoPacotesSegmento> {
        let mut filho = criar_comando(&self.caminho_ffprobe)
            .args([
                "-v",
                "error",
                "-show_packets",
                "-show_entries",
                "packet=stream_index,codec_type,pts_time,dts_time,flags",
                "-of",
                "csv=p=0",
            ])
            .arg(arquivo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|erro| ErroDivisor::io("inspeção de pacotes do segmento via ffprobe", erro))?;

        let stdout = filho
            .stdout
            .take()
            .ok_or_else(|| ErroDivisor::FalhaExecucaoFfprobe {
                detalhe: "Não foi possível abrir stdout do ffprobe.".to_string(),
            })?;

        let stderr = filho
            .stderr
            .take()
            .ok_or_else(|| ErroDivisor::FalhaExecucaoFfprobe {
                detalhe: "Não foi possível abrir stderr do ffprobe.".to_string(),
            })?;

        let coletor_stderr = thread::spawn(move || {
            let mut texto = String::new();
            let mut leitor = BufReader::new(stderr);
            let _ = leitor.read_to_string(&mut texto);
            texto
        });

        let leitor = BufReader::new(stdout);
        let mut pacotes_video = 0u64;
        let mut pacotes_audio = 0u64;
        let mut primeiro_pacote_video_eh_keyframe = false;
        let mut primeiro_pts_video: Option<f64> = None;
        let mut ultimo_pts_video: Option<f64> = None;
        let mut stream_video_detectado: Option<usize> = indice_stream_video_esperado;

        for linha_res in leitor.lines() {
            let linha = linha_res.map_err(|erro| {
                ErroDivisor::io("leitura de pacotes do segmento via ffprobe", erro)
            })?;
            let linha = linha.trim();
            if linha.is_empty() {
                continue;
            }

            // Ordem dos campos no CSV do ffprobe acompanha a definição interna do FFprobe:
            // codec_type, stream_index, pts_time, dts_time, flags (pode ter campos extras de side_data no fim)
            let campos: Vec<&str> = linha.split(',').collect();
            if campos.len() < 5 {
                continue;
            }

            let codec_type = campos[0].trim();
            let stream_idx = campos[1].trim().parse::<usize>().ok();
            let pts = campos[2].trim().parse::<f64>().ok();
            let dts = campos[3].trim().parse::<f64>().ok();
            let flags = campos[4].trim();

            if codec_type == "video" {
                let eh_video_principal = match (stream_video_detectado, stream_idx) {
                    (Some(esperado), Some(atual)) => esperado == atual,
                    (None, Some(atual)) => {
                        stream_video_detectado = Some(atual);
                        true
                    }
                    _ => true,
                };

                if eh_video_principal {
                    if pacotes_video == 0 {
                        primeiro_pacote_video_eh_keyframe = flags.contains('K');
                    }
                    pacotes_video = pacotes_video.saturating_add(1);

                    if let Some(tempo) = pts.or(dts) {
                        primeiro_pts_video = Some(match primeiro_pts_video {
                            Some(min_atual) => min_atual.min(tempo),
                            None => tempo,
                        });
                        ultimo_pts_video = Some(match ultimo_pts_video {
                            Some(max_atual) => max_atual.max(tempo),
                            None => tempo,
                        });
                    }
                }
            } else if codec_type == "audio" {
                pacotes_audio = pacotes_audio.saturating_add(1);
            }
        }

        let status = filho
            .wait()
            .map_err(|erro| ErroDivisor::io("finalização do ffprobe de inspeção", erro))?;
        let texto_stderr = coletor_stderr.join().unwrap_or_default();

        if !status.success() {
            return Err(ErroDivisor::FalhaExecucaoFfprobe {
                detalhe: if texto_stderr.trim().is_empty() {
                    format!(
                        "ffprobe retornou código {:?} para '{}'",
                        status.code(),
                        arquivo.display()
                    )
                } else {
                    texto_stderr.trim().to_string()
                },
            });
        }

        Ok(ResumoPacotesSegmento {
            pacotes_video,
            pacotes_audio,
            primeiro_pacote_video_eh_keyframe,
            primeiro_pts_video,
            ultimo_pts_video,
        })
    }

    /// Extrai uma miniatura JPEG/PNG leve a partir de um quadro do vídeo original (somente leitura),
    /// sem alterar nem recodificar o arquivo de origem.
    pub fn extrair_miniatura(
        &self,
        arquivo_video: &Path,
        instante_segundos: f64,
        destino_imagem: &Path,
        largura_maxima: u32,
    ) -> Resultado<()> {
        let tempo = instante_segundos.max(0.0);
        let filtro_escala = format!("scale='min({largura_maxima},iw)':-2");

        let mut cmd = criar_comando(&self.caminho_ffmpeg);
        cmd.args([
            "-hide_banner",
            "-nostdin",
            "-y",
            "-v",
            "error",
            "-ss",
            &format!("{tempo:.3}"),
            "-i",
        ]);
        cmd.arg(arquivo_video);
        cmd.args(["-frames:v", "1", "-vf", &filtro_escala, "-q:v", "5"]);
        cmd.arg(destino_imagem);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let saida = cmd.output().map_err(|erro| {
            ErroDivisor::io("invocação do ffmpeg para geração de miniatura", erro)
        })?;

        if !saida.status.success() {
            // Fallback no instante 0.0 caso o seek inicial caia após o último frame em vídeos curtos.
            if tempo > 0.0 {
                return self.extrair_miniatura(arquivo_video, 0.0, destino_imagem, largura_maxima);
            }
            let stderr = String::from_utf8_lossy(&saida.stderr).trim().to_string();
            return Err(ErroDivisor::ErroMiniatura {
                caminho: arquivo_video.to_path_buf(),
                detalhe: if stderr.is_empty() {
                    format!("ffmpeg retornou código {:?}", saida.status.code())
                } else {
                    stderr
                },
            });
        }

        Ok(())
    }
}

/// Constrói um `Command` seguro (sem shell), suprimindo janelas de console no Windows.
fn criar_comando(executavel: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(executavel);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (0x08000000): evita piscar janela preta do console no Windows GUI.
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// Testa os candidatos de uma ferramenta na ordem 1 -> 4 retornando o primeiro funcional.
fn diagnosticar_ferramenta_individual(
    nome_base: &str,
    #[allow(unused_variables)] caminho_personalizado: Option<&Path>,
) -> InfoFerramenta {
    #[cfg(feature = "windows-bundle")]
    {
        let resultado = crate::ferramentas_distribuidas::resolver(nome_base);
        match resultado {
            Ok(caminho) => {
                let versao = consultar_versao_executavel(&caminho);
                InfoFerramenta {
                    nome: nome_base.to_string(),
                    disponivel: versao.is_some(),
                    caminho: Some(caminho),
                    origem: Some(OrigemFerramenta::SubdiretorioFerramentas),
                    erro: if versao.is_none() {
                        Some("Ferramenta distribuída não pôde ser executada".into())
                    } else {
                        None
                    },
                    versao,
                }
            }
            Err(erro) => InfoFerramenta {
                nome: nome_base.to_string(),
                disponivel: false,
                caminho: None,
                origem: None,
                versao: None,
                erro: Some(erro.to_string()),
            },
        }
    }
    #[cfg(not(feature = "windows-bundle"))]
    diagnosticar_ferramenta_local(nome_base, caminho_personalizado)
}

#[cfg(not(feature = "windows-bundle"))]
fn diagnosticar_ferramenta_local(
    nome_base: &str,
    caminho_personalizado: Option<&Path>,
) -> InfoFerramenta {
    let candidatos = listar_candidatos_ferramenta(nome_base, caminho_personalizado, None);

    // Se o usuário informou um caminho personalizado e ele falha, não mascaramos silenciosamente
    // sem antes tentar; mas se o usuário especificou explicitamente um caminho inválido na CLI,
    // preservamos a ordem: se personalizado existir e falhar, seguimos para fallback apenas quando
    // não há caminho personalizado, ou verificamos se o caminho personalizado é obrigatório.
    if let Some(personalizado) = caminho_personalizado
        && !personalizado.as_os_str().is_empty()
    {
        if let Some(versao) = consultar_versao_executavel(personalizado) {
            return InfoFerramenta {
                nome: nome_base.to_string(),
                disponivel: true,
                caminho: Some(personalizado.to_path_buf()),
                origem: Some(OrigemFerramenta::Personalizado),
                versao: Some(versao),
                erro: None,
            };
        }
        return InfoFerramenta {
            nome: nome_base.to_string(),
            disponivel: false,
            caminho: Some(personalizado.to_path_buf()),
            origem: Some(OrigemFerramenta::Personalizado),
            versao: None,
            erro: Some(format!(
                "{nome_base} (caminho personalizado inválido: '{}')",
                personalizado.display()
            )),
        };
    }

    for (caminho, origem) in candidatos {
        if let Some(versao) = consultar_versao_executavel(&caminho) {
            return InfoFerramenta {
                nome: nome_base.to_string(),
                disponivel: true,
                caminho: Some(caminho),
                origem: Some(origem),
                versao: Some(versao),
                erro: None,
            };
        }
    }

    InfoFerramenta {
        nome: nome_base.to_string(),
        disponivel: false,
        caminho: None,
        origem: None,
        versao: None,
        erro: Some(format!(
            "{nome_base} não encontrado no diretório do aplicativo, pasta de ferramentas ou PATH."
        )),
    }
}

/// Executa `<executavel> -version` e retorna a primeira linha de versão em caso de sucesso.
fn consultar_versao_executavel(caminho: &Path) -> Option<String> {
    #[cfg(all(windows, feature = "windows-bundle"))]
    let _guards = crate::ferramentas_distribuidas::proteger_execucao(caminho).ok()?;
    let saida = criar_comando(caminho)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !saida.status.success() {
        return None;
    }

    let texto = String::from_utf8_lossy(&saida.stdout);
    let primeira_linha = texto
        .lines()
        .next()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .unwrap_or("Versão disponível")
        .to_string();

    Some(primeira_linha)
}
