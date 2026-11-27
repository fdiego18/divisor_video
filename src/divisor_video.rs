//! Orquestração completa da divisão de um único arquivo de vídeo sem perda de qualidade.
//!
//! Coordena análise de pacotes, planejamento adaptativo em keyframes, execução do FFmpeg
//! em diretório temporário seguro, recuo automático caso algum segmento ultrapasse o limite,
//! validação rigorosa e movimentação atômica dos arquivos validados para o destino final.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use tracing::{debug, info, warn};

use crate::analisador_video::{analisar_metadados, analisar_video};
use crate::configuracao::Configuracao;
use crate::erros::{ErroDivisor, Resultado};
use crate::executor_ffmpeg::{ExecutorFfmpeg, ParametrosSegmentacao};
use crate::integracao_so::verificar_espaco_para_video;
use crate::planejador_segmentos::PlanejadorSegmentos;
use crate::progresso::GerenciadorProgresso;
use crate::validador::{
    RelatorioValidacaoConjunto, SegmentoValidado, validar_conjunto_segmentos, validar_segmento,
};

/// Eventos de progresso emitidos pelo núcleo durante a divisão de um vídeo.
///
/// Independente de interface gráfica ou CLI, permitindo observação em tempo real.
#[derive(Debug, Clone)]
pub enum EventoProgressoDivisao {
    /// Verificando metadados básicos e integridade inicial do arquivo.
    VerificandoMetadados,
    /// Analisando pacotes e keyframes em streaming via FFprobe.
    AnalisandoPacotes,
    /// Segmentando via FFmpeg (`-c copy`).
    Processando {
        /// Fração estimada concluída (`0.0..=1.0`).
        fracao: f64,
        /// Número da tentativa adaptativa (base 1).
        tentativa: usize,
        /// Quantidade planejada de segmentos nesta tentativa.
        segmentos_planejados: usize,
    },
    /// Validando integridade, codecs, keyframes e continuidade dos segmentos gerados.
    Validando {
        /// Parte atual em validação (base 1).
        parte_atual: usize,
        /// Quantidade total de partes a validar.
        total_partes: usize,
    },
}

/// Estado final do processamento de um único arquivo de vídeo.
#[derive(Debug, Clone)]
pub enum StatusProcessamentoVideo {
    /// Vídeo foi dividido e todos os segmentos foram validados abaixo do limite.
    Dividido {
        /// Diretório onde os segmentos finais foram gravados.
        diretorio_destino: PathBuf,
        /// Lista ordenada de segmentos gerados e validados.
        segmentos: Vec<SegmentoValidado>,
        /// Relatório consolidado da validação global.
        validacao: RelatorioValidacaoConjunto,
        /// Quantidade de tentativas executadas até convergir.
        tentativas: usize,
        /// Indica se o vídeo foi classificado como VBR.
        eh_vbr: bool,
    },
    /// Vídeo já possui tamanho menor ou igual ao limite máximo configurado.
    IgnoradoAbaixoDoLimite {
        /// Limite máximo configurado.
        limite_bytes: u64,
    },
}

/// Relatório completo do processamento de um vídeo individual.
#[derive(Debug, Clone)]
pub struct RelatorioVideo {
    /// Caminho do arquivo original (preservado intacto).
    pub arquivo_original: PathBuf,
    /// Tamanho do arquivo original em bytes.
    pub tamanho_original_bytes: u64,
    /// Tempo total decorrido no processamento deste vídeo.
    pub tempo_decorrido: Duration,
    /// Resultado obtido (dividido ou ignorado).
    pub status: StatusProcessamentoVideo,
}

/// Divide um único arquivo de vídeo respeitando rigorosamente todas as regras de tamanho e qualidade,
/// reportando progresso para a barra CLI (`GerenciadorProgresso`).
pub fn dividir_arquivo_video(
    caminho_video: &Path,
    config: &Configuracao,
    executor: &ExecutorFfmpeg,
    progresso: &GerenciadorProgresso,
    cancelamento: &Arc<AtomicBool>,
) -> Resultado<RelatorioVideo> {
    if cancelamento.load(Ordering::Relaxed) {
        return Err(ErroDivisor::CanceladoPeloUsuario);
    }

    if !caminho_video.is_file() {
        return Err(ErroDivisor::ArquivoNaoEncontrado {
            caminho: caminho_video.to_path_buf(),
        });
    }

    let tamanho_original_bytes = fs::metadata(caminho_video)
        .map_err(|erro| {
            ErroDivisor::io(
                format!("leitura de metadados de '{}'", caminho_video.display()),
                erro,
            )
        })?
        .len();

    let nome_arquivo = caminho_video
        .file_name()
        .map_or_else(|| "video".to_string(), |n| n.to_string_lossy().into_owned());

    let barra = progresso.criar_barra_video(&nome_arquivo, tamanho_original_bytes);

    let resultado = dividir_arquivo_video_com_observador(
        caminho_video,
        config,
        executor,
        cancelamento,
        |evento| match evento {
            EventoProgressoDivisao::VerificandoMetadados => {
                barra.definir_mensagem("verificando metadados...");
            }
            EventoProgressoDivisao::AnalisandoPacotes => {
                barra.definir_mensagem("analisando keyframes e pacotes...");
            }
            EventoProgressoDivisao::Processando {
                fracao,
                tentativa,
                segmentos_planejados,
            } => {
                if tentativa == 1 {
                    barra.definir_mensagem(format!(
                        "dividindo em ~{segmentos_planejados} partes..."
                    ));
                } else {
                    barra.definir_mensagem(format!("ajustando cortes (tentativa {tentativa})..."));
                }
                barra.atualizar_fracao(fracao);
            }
            EventoProgressoDivisao::Validando {
                parte_atual,
                total_partes,
            } => {
                barra.definir_mensagem(format!("validando {parte_atual}/{total_partes} partes..."));
            }
        },
    );

    match &resultado {
        Ok(relatorio) => match &relatorio.status {
            StatusProcessamentoVideo::Dividido { segmentos, .. } => {
                barra.concluir_sucesso(format!("concluído ({} partes)", segmentos.len()));
            }
            StatusProcessamentoVideo::IgnoradoAbaixoDoLimite { .. } => {
                barra.concluir_sucesso("ignorado (<= limite)");
            }
        },
        Err(ErroDivisor::CanceladoPeloUsuario) => {
            barra.finalizar_com_mensagem("cancelado");
        }
        Err(_) => {
            barra.finalizar_com_mensagem("falhou");
        }
    }

    resultado
}

/// Divide um único arquivo de vídeo emitindo eventos estruturados de progresso via `ao_evento`.
///
/// Ideal para uso pela interface desktop, filas assíncronas ou integrações programáticas,
/// preservando 100% da lógica de divisão, recuo, validação e publicação atômica.
pub fn dividir_arquivo_video_com_observador<F>(
    caminho_video: &Path,
    config: &Configuracao,
    executor: &ExecutorFfmpeg,
    cancelamento: &Arc<AtomicBool>,
    mut ao_evento: F,
) -> Resultado<RelatorioVideo>
where
    F: FnMut(EventoProgressoDivisao),
{
    let inicio = Instant::now();

    if cancelamento.load(Ordering::Relaxed) {
        return Err(ErroDivisor::CanceladoPeloUsuario);
    }

    if !caminho_video.is_file() {
        return Err(ErroDivisor::ArquivoNaoEncontrado {
            caminho: caminho_video.to_path_buf(),
        });
    }

    let tamanho_original_bytes = fs::metadata(caminho_video)
        .map_err(|erro| {
            ErroDivisor::io(
                format!("leitura de metadados de '{}'", caminho_video.display()),
                erro,
            )
        })?
        .len();

    executar_fluxo_divisao(
        caminho_video,
        tamanho_original_bytes,
        config,
        executor,
        &mut ao_evento,
        cancelamento,
        inicio,
    )
}

/// Fluxo interno completo de análise, divisão adaptativa, validação e publicação.
fn executar_fluxo_divisao<F>(
    caminho_video: &Path,
    tamanho_original_bytes: u64,
    config: &Configuracao,
    executor: &ExecutorFfmpeg,
    ao_evento: &mut F,
    cancelamento: &Arc<AtomicBool>,
    inicio: Instant,
) -> Resultado<RelatorioVideo>
where
    F: FnMut(EventoProgressoDivisao),
{
    // Primeiro valida a integridade básica dos metadados mesmo para arquivos pequenos,
    // assegurando que arquivos corrompidos sejam reportados como erro e não ignorados.
    ao_evento(EventoProgressoDivisao::VerificandoMetadados);
    let _metadados_iniciais = analisar_metadados(executor, caminho_video)?;

    if tamanho_original_bytes <= config.tamanho_maximo_bytes {
        info!(
            arquivo = %caminho_video.display(),
            tamanho_bytes = tamanho_original_bytes,
            limite_bytes = config.tamanho_maximo_bytes,
            "Vídeo já está abaixo ou igual ao limite máximo; ignorado."
        );
        return Ok(RelatorioVideo {
            arquivo_original: caminho_video.to_path_buf(),
            tamanho_original_bytes,
            tempo_decorrido: inicio.elapsed(),
            status: StatusProcessamentoVideo::IgnoradoAbaixoDoLimite {
                limite_bytes: config.tamanho_maximo_bytes,
            },
        });
    }

    // Extrai nome base e extensão do arquivo original preservando o contêiner original.
    let nome_base = caminho_video
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "video".to_string());

    let extensao = caminho_video
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "mp4".to_string());

    let diretorio_destino = config.diretorio_saida.join(&nome_base);

    // Verifica antecipadamente a política de sobrescrita antes de processar.
    if !config.permitir_sobrescrita && diretorio_destino.exists() {
        verificar_conflito_sobrescrita(&diretorio_destino, &nome_base, &extensao)?;
    }

    // Garante existência do diretório raiz de saída.
    fs::create_dir_all(&config.diretorio_saida).map_err(|erro| {
        ErroDivisor::io(
            format!(
                "criação do diretório de saída '{}'",
                config.diretorio_saida.display()
            ),
            erro,
        )
    })?;

    // Verifica espaço disponível em disco no volume de saída antes de gravar temporários.
    verificar_espaco_para_video(&config.diretorio_saida, tamanho_original_bytes)?;

    // Etapa 1: Análise completa de pacotes e keyframes em streaming via FFprobe.
    ao_evento(EventoProgressoDivisao::AnalisandoPacotes);
    let mapa = analisar_video(executor, caminho_video, cancelamento)?;

    // Cria diretório temporário seguro (removido automaticamente em caso de falha/cancelamento).
    let dir_temporario = criar_diretorio_temporario(&config.diretorio_saida)?;

    // Etapa 2: Planejamento dinâmico + geração + verificação de limite com recuo adaptativo.
    let mut planejador = PlanejadorSegmentos::novo(
        &mapa,
        config.tamanho_maximo_bytes,
        config.tamanho_alvo_bytes,
    );
    let mut tetos_por_bloco_inicio: HashMap<usize, usize> = HashMap::new();
    let max_tentativas = mapa.blocos_gop.len().clamp(12, 64);

    let mut segmentos_validados_finais: Option<(
        Vec<SegmentoValidado>,
        RelatorioValidacaoConjunto,
        usize,
    )> = None;

    for tentativa in 1..=max_tentativas {
        if cancelamento.load(Ordering::Relaxed) {
            return Err(ErroDivisor::CanceladoPeloUsuario);
        }

        let plano = planejador.planejar(&mapa, &tetos_por_bloco_inicio)?;
        debug!(
            arquivo = %caminho_video.display(),
            tentativa,
            segmentos_planejados = plano.segmentos.len(),
            "Plano de divisão calculado"
        );

        let dir_tentativa = dir_temporario.path().join(format!("tentativa_{tentativa}"));
        fs::create_dir_all(&dir_tentativa).map_err(|erro| {
            ErroDivisor::io("criação de subdiretório temporário da tentativa", erro)
        })?;

        let padrao_saida = dir_tentativa.join(format!("seg_%03d.{extensao}"));
        let params = ParametrosSegmentacao {
            arquivo_entrada: caminho_video,
            padrao_saida: &padrao_saida,
            extensao_saida: &extensao,
            indices_streams: &mapa.metadados.indices_streams_mapeados,
            tempos_corte_segundos: &plano.tempos_corte_ffmpeg,
        };

        let segmentos_planejados = plano.segmentos.len();
        ao_evento(EventoProgressoDivisao::Processando {
            fracao: 0.0,
            tentativa,
            segmentos_planejados,
        });

        let duracao_total = mapa.metadados.duracao_segundos;
        executor.executar_segmentacao(&params, cancelamento, |segundos_atuais| {
            let fracao = if duracao_total > 0.0 {
                (segundos_atuais / duracao_total).clamp(0.0, 1.0)
            } else {
                0.0
            };
            ao_evento(EventoProgressoDivisao::Processando {
                fracao,
                tentativa,
                segmentos_planejados,
            });
        })?;

        let arquivos_gerados = listar_segmentos_gerados(&dir_tentativa, "seg_", &extensao)?;
        if arquivos_gerados.is_empty() {
            return Err(ErroDivisor::FalhaValidacao {
                detalhe: "FFmpeg concluiu sem gerar nenhum arquivo de segmento.".to_string(),
            });
        }

        // Verifica o tamanho real de TODOS os arquivos gerados antes da validação pesada.
        let mut tamanhos_reais = Vec::with_capacity(arquivos_gerados.len());
        let mut primeiro_excedente: Option<(usize, u64)> = None;

        for (idx, caminho_seg) in arquivos_gerados.iter().enumerate() {
            let tamanho = fs::metadata(caminho_seg)
                .map_err(|erro| {
                    ErroDivisor::io(
                        format!("verificação do tamanho de '{}'", caminho_seg.display()),
                        erro,
                    )
                })?
                .len();
            tamanhos_reais.push(tamanho);
            if tamanho > config.tamanho_maximo_bytes && primeiro_excedente.is_none() {
                primeiro_excedente = Some((idx, tamanho));
            }
        }

        // Calibra o estimador de overhead quando a quantidade de arquivos coincide com o plano.
        if arquivos_gerados.len() == plano.segmentos.len() {
            for (seg_plan, &tam_real) in plano.segmentos.iter().zip(tamanhos_reais.iter()) {
                planejador.estimador_mut().calibrar_com_medicao(
                    seg_plan.bytes_pacotes,
                    seg_plan.total_pacotes,
                    tam_real,
                );
            }
        }

        if let Some((idx_excedente, tamanho_excedente)) = primeiro_excedente {
            warn!(
                arquivo = %caminho_video.display(),
                tentativa,
                segmento = idx_excedente + 1,
                tamanho_real = tamanho_excedente,
                limite = config.tamanho_maximo_bytes,
                "Segmento ultrapassou o limite máximo; selecionando keyframe anterior e reprocessando."
            );

            // Remove imediatamente os arquivos desta tentativa inválida.
            let _ = fs::remove_dir_all(&dir_tentativa);

            if let Some(seg_plan) = plano.segmentos.get(idx_excedente) {
                let novo_fim =
                    planejador.calcular_recuo_bloco(&mapa, seg_plan, tamanho_excedente)?;
                tetos_por_bloco_inicio.insert(seg_plan.bloco_inicio, novo_fim);
            } else if let Some(ultimo_seg) = plano.segmentos.last() {
                let novo_fim =
                    planejador.calcular_recuo_bloco(&mapa, ultimo_seg, tamanho_excedente)?;
                tetos_por_bloco_inicio.insert(ultimo_seg.bloco_inicio, novo_fim);
            } else {
                return Err(ErroDivisor::SegmentoExcedeuLimite {
                    indice: idx_excedente + 1,
                    tamanho_bytes: tamanho_excedente,
                    limite_bytes: config.tamanho_maximo_bytes,
                });
            }

            continue;
        }

        // Todos os arquivos respeitam o limite máximo! Executa validação rigorosa de integridade.
        let total_partes = arquivos_gerados.len();
        let mut validados = Vec::with_capacity(total_partes);

        for (idx, caminho_seg) in arquivos_gerados.iter().enumerate() {
            if cancelamento.load(Ordering::Relaxed) {
                return Err(ErroDivisor::CanceladoPeloUsuario);
            }
            ao_evento(EventoProgressoDivisao::Validando {
                parte_atual: idx + 1,
                total_partes,
            });
            let seg_val = validar_segmento(
                executor,
                &mapa.metadados,
                caminho_seg,
                idx + 1,
                config.tamanho_maximo_bytes,
            )?;
            validados.push(seg_val);
        }

        let relatorio_conjunto =
            validar_conjunto_segmentos(&mapa, &validados, config.tamanho_maximo_bytes)?;

        segmentos_validados_finais = Some((validados, relatorio_conjunto, tentativa));
        break;
    }

    let (mut segmentos_validados, relatorio_validacao, tentativas) =
        segmentos_validados_finais.ok_or_else(|| ErroDivisor::FalhaValidacao {
            detalhe: format!(
                "Não foi possível convergir para segmentos dentro do limite de {} bytes após {} tentativas.",
                config.tamanho_maximo_bytes, max_tentativas
            ),
        })?;

    // Etapa 3: Publicação atômica no diretório final `<saida>/<nome_base>/`.
    fs::create_dir_all(&diretorio_destino).map_err(|erro| {
        ErroDivisor::io(
            format!(
                "criação do diretório final '{}'",
                diretorio_destino.display()
            ),
            erro,
        )
    })?;

    // Remove partes antigas somente se sobrescrita foi explicitamente autorizada.
    if config.permitir_sobrescrita {
        remover_partes_antigas(&diretorio_destino, &nome_base, &extensao)?;
    }

    for seg in &mut segmentos_validados {
        let nome_final = format!("{nome_base}_parte_{:03}.{extensao}", seg.indice);
        let caminho_final = diretorio_destino.join(nome_final);

        if caminho_final.exists() && !config.permitir_sobrescrita {
            return Err(ErroDivisor::SobrescritaNaoAutorizada {
                caminho: caminho_final,
            });
        }

        mover_arquivo_seguro(&seg.caminho, &caminho_final)?;
        seg.caminho = caminho_final;
    }

    // `dir_temporario` é limpo automaticamente ao sair de escopo.
    drop(dir_temporario);

    info!(
        arquivo = %caminho_video.display(),
        partes = segmentos_validados.len(),
        maior_parte = relatorio_validacao.maior_segmento_bytes,
        tentativas,
        "Vídeo dividido e validado com sucesso"
    );

    Ok(RelatorioVideo {
        arquivo_original: caminho_video.to_path_buf(),
        tamanho_original_bytes,
        tempo_decorrido: inicio.elapsed(),
        status: StatusProcessamentoVideo::Dividido {
            diretorio_destino,
            segmentos: segmentos_validados,
            validacao: relatorio_validacao,
            tentativas,
            eh_vbr: mapa.eh_vbr,
        },
    })
}

/// Cria diretório temporário preferencialmente no mesmo filesystem do diretório de saída
/// (permitindo `rename` atômico) com fallback para o diretório temporário do sistema.
fn criar_diretorio_temporario(diretorio_base: &Path) -> Resultado<TempDir> {
    match tempfile::Builder::new()
        .prefix(".divisor_tmp_")
        .tempdir_in(diretorio_base)
    {
        Ok(dir) => Ok(dir),
        Err(_) => tempfile::Builder::new()
            .prefix("divisor_video_tmp_")
            .tempdir()
            .map_err(|erro| ErroDivisor::io("criação de diretório temporário seguro", erro)),
    }
}

/// Verifica se já existem arquivos de partes no diretório de destino quando sobrescrita está desabilitada.
fn verificar_conflito_sobrescrita(
    diretorio_destino: &Path,
    nome_base: &str,
    extensao: &str,
) -> Resultado<()> {
    let prefixo = format!("{nome_base}_parte_");
    let sufixo = format!(".{extensao}");

    let entradas = fs::read_dir(diretorio_destino).map_err(|erro| {
        ErroDivisor::io(
            format!(
                "leitura do diretório de destino '{}'",
                diretorio_destino.display()
            ),
            erro,
        )
    })?;

    for entrada_res in entradas {
        let entrada = entrada_res
            .map_err(|erro| ErroDivisor::io("iteração do diretório de destino", erro))?;
        let nome = entrada.file_name().to_string_lossy().into_owned();
        if nome.starts_with(&prefixo) && nome.to_ascii_lowercase().ends_with(&sufixo) {
            return Err(ErroDivisor::SobrescritaNaoAutorizada {
                caminho: entrada.path(),
            });
        }
    }

    Ok(())
}

/// Remove segmentos antigos correspondentes ao mesmo vídeo quando `--sobrescrever` está ativo.
fn remover_partes_antigas(
    diretorio_destino: &Path,
    nome_base: &str,
    extensao: &str,
) -> Resultado<()> {
    let prefixo = format!("{nome_base}_parte_");
    let sufixo = format!(".{extensao}");

    let entradas = fs::read_dir(diretorio_destino).map_err(|erro| {
        ErroDivisor::io(
            format!(
                "leitura do diretório '{}' para limpeza de sobrescrita",
                diretorio_destino.display()
            ),
            erro,
        )
    })?;

    for entrada_res in entradas {
        let entrada = entrada_res.map_err(|erro| {
            ErroDivisor::io("iteração do diretório para limpeza de sobrescrita", erro)
        })?;
        let nome = entrada.file_name().to_string_lossy().into_owned();
        if nome.starts_with(&prefixo) && nome.to_ascii_lowercase().ends_with(&sufixo) {
            fs::remove_file(entrada.path()).map_err(|erro| {
                ErroDivisor::io(
                    format!("remoção do arquivo antigo '{}'", entrada.path().display()),
                    erro,
                )
            })?;
        }
    }

    Ok(())
}

/// Lista e ordena numericamente os segmentos gerados pelo FFmpeg no diretório da tentativa.
fn listar_segmentos_gerados(
    dir_tentativa: &Path,
    prefixo: &str,
    extensao: &str,
) -> Resultado<Vec<PathBuf>> {
    let sufixo = format!(".{extensao}");
    let mut encontrados: Vec<(usize, PathBuf)> = Vec::new();

    let entradas = fs::read_dir(dir_tentativa).map_err(|erro| {
        ErroDivisor::io(
            format!(
                "leitura do diretório temporário '{}'",
                dir_tentativa.display()
            ),
            erro,
        )
    })?;

    for entrada_res in entradas {
        let entrada =
            entrada_res.map_err(|erro| ErroDivisor::io("leitura de entrada temporária", erro))?;
        let caminho = entrada.path();
        if !caminho.is_file() {
            continue;
        }
        let nome = entrada.file_name().to_string_lossy().into_owned();
        if let Some(restante) = nome.strip_prefix(prefixo)
            && let Some(numero_str) = restante.strip_suffix(&sufixo)
            && let Ok(numero) = numero_str.parse::<usize>()
        {
            encontrados.push((numero, caminho));
        }
    }

    encontrados.sort_by_key(|(num, _)| *num);
    Ok(encontrados.into_iter().map(|(_, p)| p).collect())
}

/// Move um arquivo de forma atômica quando no mesmo filesystem, ou via cópia + remoção
/// segura caso origem e destino estejam em volumes distintos.
fn mover_arquivo_seguro(origem: &Path, destino: &Path) -> Resultado<()> {
    match fs::rename(origem, destino) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(origem, destino).map_err(|erro| {
                ErroDivisor::io(
                    format!(
                        "cópia do segmento validado '{}' para '{}'",
                        origem.display(),
                        destino.display()
                    ),
                    erro,
                )
            })?;
            fs::remove_file(origem).map_err(|erro| {
                ErroDivisor::io(
                    format!("remoção do temporário '{}'", origem.display()),
                    erro,
                )
            })?;
            Ok(())
        }
    }
}
