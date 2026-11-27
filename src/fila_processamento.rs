//! Motor assíncrono de análise em segundo plano e fila de processamento paralelo controlado.
//!
//! Mantém a thread da interface gráfica 100% livre de operações demoradas, suporta
//! adição de novos vídeos durante o processamento, cancelamento individual ou global,
//! throttling de progresso para evitar polling excessivo e encerramento seguro de filhos.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tracing::{debug, warn};

use crate::analisador_video::analisar_metadados;
use crate::configuracao::Configuracao;
use crate::divisor_video::{
    EventoProgressoDivisao, StatusProcessamentoVideo, dividir_arquivo_video_com_observador,
};
use crate::erros::ErroDivisor;
use crate::eventos::{
    EstadoItemFila, EventoAplicacao, InfoSegmentoGerado, ResultadoFinalItem, ResumoExecucaoFila,
    ResumoTecnicoVideo,
};
use crate::executor_ffmpeg::ExecutorFfmpeg;
use crate::miniaturas::GerenciadorMiniaturas;

/// Intervalo mínimo entre emissões consecutivas de progresso do mesmo item (evita excesso de repaints).
const INTERVALO_THROTTLE_PROGRESSO: Duration = Duration::from_millis(80);

/// Tarefa enfileirada para análise técnica e geração de miniatura em segundo plano.
#[derive(Debug, Clone)]
pub struct TarefaAnalise {
    pub id_item: u64,
    pub caminho: PathBuf,
    pub executor: ExecutorFfmpeg,
    pub miniaturas: GerenciadorMiniaturas,
}

/// Tarefa enfileirada para divisão efetiva de um vídeo.
#[derive(Debug, Clone)]
pub struct TarefaDivisao {
    pub id_item: u64,
    pub caminho: PathBuf,
    pub tamanho_original_bytes: u64,
    pub diretorio_saida: PathBuf,
    pub tamanho_maximo_bytes: u64,
    pub tamanho_alvo_bytes: u64,
    pub permitir_sobrescrita: bool,
    pub executor: ExecutorFfmpeg,
}

/// Acumulador interno thread-safe de estatísticas do ciclo atual da fila.
#[derive(Default)]
struct EstatisticasCiclo {
    videos_divididos: usize,
    videos_ignorados: usize,
    segmentos_gerados: usize,
    falhas: usize,
    cancelados: usize,
    bytes_processados: u64,
}

/// Estado compartilhado entre a thread da interface e os workers de processamento.
struct EstadoCompartilhadoFila {
    pendentes: Mutex<VecDeque<TarefaDivisao>>,
    condvar: Condvar,
    cancelamentos_individuais: Mutex<HashMap<u64, Arc<AtomicBool>>>,
    cancelamento_global: Arc<AtomicBool>,
    processamento_ativo: AtomicBool,
    workers_ocupados: AtomicUsize,
    estatisticas: Mutex<EstatisticasCiclo>,
    inicio_ciclo: Mutex<Option<Instant>>,
}

/// Controlador assíncrono de análise técnica e processamento de vídeos em segundo plano.
pub struct MotorFilaProcessamento {
    tx_eventos: Sender<EventoAplicacao>,
    rx_eventos: Receiver<EventoAplicacao>,
    tx_analise: Sender<TarefaAnalise>,
    estado_fila: Arc<EstadoCompartilhadoFila>,
    acordar_ui: Arc<dyn Fn() + Send + Sync>,
}

impl MotorFilaProcessamento {
    /// Inicializa o motor assíncrono com uma função opcional para acordar a interface gráfica
    /// (`egui::Context::request_repaint`) sempre que novos eventos chegarem.
    pub fn novo(acordar_ui: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx_eventos, rx_eventos) = mpsc::channel();
        let (tx_analise, rx_analise) = mpsc::channel::<TarefaAnalise>();

        let estado_fila = Arc::new(EstadoCompartilhadoFila {
            pendentes: Mutex::new(VecDeque::new()),
            condvar: Condvar::new(),
            cancelamentos_individuais: Mutex::new(HashMap::new()),
            cancelamento_global: Arc::new(AtomicBool::new(false)),
            processamento_ativo: AtomicBool::new(false),
            workers_ocupados: AtomicUsize::new(0),
            estatisticas: Mutex::new(EstatisticasCiclo::default()),
            inicio_ciclo: Mutex::new(None),
        });

        // Inicia worker dedicado de análise técnica e geração de miniaturas em segundo plano.
        let tx_ev_analise = tx_eventos.clone();
        let acordar_analise = Arc::clone(&acordar_ui);
        let _ = thread::Builder::new()
            .name("divisor-analise-bg".to_string())
            .spawn(move || {
                loop_worker_analise(rx_analise, tx_ev_analise, acordar_analise);
            });

        Self {
            tx_eventos,
            rx_eventos,
            tx_analise,
            estado_fila,
            acordar_ui,
        }
    }

    /// Enfileira um vídeo para análise de metadados e geração de miniatura em segundo plano.
    pub fn solicitar_analise(&self, tarefa: TarefaAnalise) {
        let _ = self.tx_analise.send(tarefa);
    }

    /// Retorna `true` se houver um ciclo de processamento ativo em execução.
    pub fn esta_processando(&self) -> bool {
        self.estado_fila.processamento_ativo.load(Ordering::SeqCst)
    }

    /// Inicia o processamento paralelo de uma lista de tarefas com `concorrencia` workers.
    ///
    /// Caso um ciclo já esteja ativo, apenas enfileira as novas tarefas dinamicamente!
    pub fn iniciar_ou_enfileirar(&self, tarefas: Vec<TarefaDivisao>, concorrencia: usize) {
        if tarefas.is_empty() {
            return;
        }

        let ja_ativo = self
            .estado_fila
            .processamento_ativo
            .swap(true, Ordering::SeqCst);

        // Registra flags individuais de cancelamento e coloca tarefas na fila pendente.
        {
            let mut mapa_cancel = self
                .estado_fila
                .cancelamentos_individuais
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut fila = self
                .estado_fila
                .pendentes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            for tarefa in tarefas {
                mapa_cancel.insert(tarefa.id_item, Arc::new(AtomicBool::new(false)));
                fila.push_back(tarefa);
            }
        }

        if ja_ativo {
            self.estado_fila.condvar.notify_all();
            return;
        }

        // Novo ciclo: limpa flag global de cancelamento e estatísticas anteriores.
        self.estado_fila
            .cancelamento_global
            .store(false, Ordering::SeqCst);
        {
            let mut stats = self
                .estado_fila
                .estatisticas
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *stats = EstatisticasCiclo::default();
        }
        {
            let mut inicio = self
                .estado_fila
                .inicio_ciclo
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *inicio = Some(Instant::now());
        }

        let qtd_workers = concorrencia.clamp(1, 16);
        let estado_coord = Arc::clone(&self.estado_fila);
        let tx_coord = self.tx_eventos.clone();
        let acordar_coord = Arc::clone(&self.acordar_ui);

        let _ = thread::Builder::new()
            .name("divisor-coordenador-fila".to_string())
            .spawn(move || {
                executar_ciclo_coordenado(estado_coord, qtd_workers, tx_coord, acordar_coord);
            });
    }

    /// Cancela o processamento de um vídeo específico (esteja ele pendente ou em andamento).
    pub fn cancelar_item(&self, id_item: u64) {
        if let Ok(mapa) = self.estado_fila.cancelamentos_individuais.lock()
            && let Some(flag) = mapa.get(&id_item)
        {
            flag.store(true, Ordering::SeqCst);
        }

        // Remove da fila de pendentes se ainda não tiver iniciado, emitindo evento de cancelamento.
        let removido_pendente = {
            let mut fila = self
                .estado_fila
                .pendentes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(pos) = fila.iter().position(|t| t.id_item == id_item) {
                fila.remove(pos)
            } else {
                None
            }
        };

        if let Some(tarefa) = removido_pendente {
            if let Ok(mut stats) = self.estado_fila.estatisticas.lock() {
                stats.cancelados += 1;
            }
            enviar_evento(
                &self.tx_eventos,
                &self.acordar_ui,
                EventoAplicacao::ItemFinalizado {
                    id_item,
                    resultado: ResultadoFinalItem {
                        estado: EstadoItemFila::Cancelado,
                        diretorio_saida: None,
                        segmentos: Vec::new(),
                        tempo_decorrido: Duration::ZERO,
                        velocidade_mb_s: 0.0,
                        mensagem_erro: None,
                        limite_utilizado_bytes: tarefa.tamanho_maximo_bytes,
                    },
                },
            );
        }
    }

    /// Cancela imediatamente todos os vídeos ativos e pendentes da fila com segurança.
    pub fn cancelar_tudo(&self) {
        self.estado_fila
            .cancelamento_global
            .store(true, Ordering::SeqCst);

        if let Ok(mapa) = self.estado_fila.cancelamentos_individuais.lock() {
            for flag in mapa.values() {
                flag.store(true, Ordering::SeqCst);
            }
        }

        // Drena todos os itens que ainda aguardavam na fila pendente e notifica cancelamento.
        let pendentes_drenados: Vec<TarefaDivisao> = {
            let mut fila = self
                .estado_fila
                .pendentes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            fila.drain(..).collect()
        };

        for tarefa in pendentes_drenados {
            if let Ok(mut stats) = self.estado_fila.estatisticas.lock() {
                stats.cancelados += 1;
            }
            enviar_evento(
                &self.tx_eventos,
                &self.acordar_ui,
                EventoAplicacao::ItemFinalizado {
                    id_item: tarefa.id_item,
                    resultado: ResultadoFinalItem {
                        estado: EstadoItemFila::Cancelado,
                        diretorio_saida: None,
                        segmentos: Vec::new(),
                        tempo_decorrido: Duration::ZERO,
                        velocidade_mb_s: 0.0,
                        mensagem_erro: None,
                        limite_utilizado_bytes: tarefa.tamanho_maximo_bytes,
                    },
                },
            );
        }

        self.estado_fila.condvar.notify_all();
    }

    /// Drena todos os eventos pendentes produzidos pelas threads de fundo sem bloquear.
    pub fn drenar_eventos(&self) -> Vec<EventoAplicacao> {
        let mut eventos = Vec::new();
        while let Ok(ev) = self.rx_eventos.try_recv() {
            eventos.push(ev);
        }
        eventos
    }
}

impl Drop for MotorFilaProcessamento {
    fn drop(&mut self) {
        self.cancelar_tudo();
    }
}

/// Loop da thread de análise técnica e geração de miniaturas em segundo plano.
fn loop_worker_analise(
    rx: Receiver<TarefaAnalise>,
    tx_eventos: Sender<EventoAplicacao>,
    acordar_ui: Arc<dyn Fn() + Send + Sync>,
) {
    while let Ok(tarefa) = rx.recv() {
        enviar_evento(
            &tx_eventos,
            &acordar_ui,
            EventoAplicacao::AnaliseIniciada {
                id_item: tarefa.id_item,
            },
        );

        match analisar_metadados(&tarefa.executor, &tarefa.caminho) {
            Ok(meta) => {
                let video = meta.stream_video_principal();
                let codecs_audio = if meta.streams_audio.is_empty() {
                    "Sem áudio".to_string()
                } else {
                    let mut unicos: Vec<String> = Vec::new();
                    for a in &meta.streams_audio {
                        if !unicos.contains(&a.codec_nome) {
                            unicos.push(a.codec_nome.clone());
                        }
                    }
                    unicos.join(", ")
                };

                let resumo = ResumoTecnicoVideo {
                    tamanho_bytes: meta.tamanho_bytes,
                    duracao_segundos: meta.duracao_segundos,
                    largura: video.map_or(0, |v| v.largura),
                    altura: video.map_or(0, |v| v.altura),
                    fps: video.map_or(0.0, |v| v.fps),
                    codec_video: video
                        .map_or_else(|| "desconhecido".to_string(), |v| v.codec_nome.clone()),
                    codec_audio: codecs_audio,
                    quantidade_faixas_audio: meta.streams_audio.len(),
                    formato: meta.formato_nome.clone(),
                };

                let duracao = resumo.duracao_segundos;
                enviar_evento(
                    &tx_eventos,
                    &acordar_ui,
                    EventoAplicacao::AnaliseConcluida {
                        id_item: tarefa.id_item,
                        resumo,
                    },
                );

                // Gera ou recupera do cache local a miniatura em segundo plano.
                match tarefa
                    .miniaturas
                    .obter_ou_gerar(&tarefa.executor, &tarefa.caminho, duracao)
                {
                    Ok(caminho_imagem) => {
                        enviar_evento(
                            &tx_eventos,
                            &acordar_ui,
                            EventoAplicacao::MiniaturaPronta {
                                id_item: tarefa.id_item,
                                caminho_imagem,
                            },
                        );
                    }
                    Err(erro) => {
                        debug!(
                            arquivo = %tarefa.caminho.display(),
                            %erro,
                            "Não foi possível gerar miniatura de pré-visualização"
                        );
                    }
                }
            }
            Err(erro) => {
                enviar_evento(
                    &tx_eventos,
                    &acordar_ui,
                    EventoAplicacao::AnaliseFalhou {
                        id_item: tarefa.id_item,
                        erro: erro.to_string(),
                    },
                );
            }
        }
    }
}

/// Coordena um conjunto controlado de `qtd_workers` threads paralelas até drenar toda a fila.
fn executar_ciclo_coordenado(
    estado: Arc<EstadoCompartilhadoFila>,
    qtd_workers: usize,
    tx_eventos: Sender<EventoAplicacao>,
    acordar_ui: Arc<dyn Fn() + Send + Sync>,
) {
    let mut handles = Vec::with_capacity(qtd_workers);

    for idx in 0..qtd_workers {
        let estado_w = Arc::clone(&estado);
        let tx_w = tx_eventos.clone();
        let acordar_w = Arc::clone(&acordar_ui);

        if let Ok(handle) = thread::Builder::new()
            .name(format!("divisor-worker-gui-{idx}"))
            .spawn(move || {
                loop_worker_divisao(estado_w, tx_w, acordar_w);
            })
        {
            handles.push(handle);
        }
    }

    for h in handles {
        let _ = h.join();
    }

    let tempo_total = estado
        .inicio_ciclo
        .lock()
        .ok()
        .and_then(|g| *g)
        .map_or(Duration::ZERO, |ini| ini.elapsed());

    let cancelada = estado.cancelamento_global.load(Ordering::SeqCst);
    let resumo = {
        let stats = estado
            .estatisticas
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let segs_total = tempo_total.as_secs_f64();
        #[allow(clippy::cast_precision_loss)]
        let vel_media = if segs_total > 0.001 {
            (stats.bytes_processados as f64 / 1_000_000.0) / segs_total
        } else {
            0.0
        };
        ResumoExecucaoFila {
            videos_processados: stats.videos_divididos + stats.videos_ignorados,
            videos_divididos: stats.videos_divididos,
            videos_ignorados: stats.videos_ignorados,
            segmentos_gerados: stats.segmentos_gerados,
            falhas: stats.falhas,
            cancelados: stats.cancelados,
            tempo_total,
            velocidade_media_mb_s: vel_media,
        }
    };

    if let Ok(mut mapa) = estado.cancelamentos_individuais.lock() {
        mapa.clear();
    }
    estado.processamento_ativo.store(false, Ordering::SeqCst);

    enviar_evento(
        &tx_eventos,
        &acordar_ui,
        EventoAplicacao::FilaFinalizada { resumo, cancelada },
    );
}

/// Worker individual que consome tarefas da fila compartilhada enquanto houver itens pendentes.
fn loop_worker_divisao(
    estado: Arc<EstadoCompartilhadoFila>,
    tx_eventos: Sender<EventoAplicacao>,
    acordar_ui: Arc<dyn Fn() + Send + Sync>,
) {
    loop {
        let proxima_tarefa = {
            let mut fila = estado
                .pendentes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            loop {
                if estado.cancelamento_global.load(Ordering::SeqCst) {
                    break None;
                }
                if let Some(tarefa) = fila.pop_front() {
                    estado.workers_ocupados.fetch_add(1, Ordering::SeqCst);
                    break Some(tarefa);
                }
                // Se ninguém está trabalhando e a fila está vazia, encerramos o ciclo.
                if estado.workers_ocupados.load(Ordering::SeqCst) == 0 {
                    estado.condvar.notify_all();
                    break None;
                }
                // Aguarda brevemente novas tarefas adicionadas durante o processamento ou fim dos outros workers.
                let (guard, _) = estado
                    .condvar
                    .wait_timeout(fila, Duration::from_millis(120))
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                fila = guard;
            }
        };

        let Some(tarefa) = proxima_tarefa else {
            break;
        };

        executar_tarefa_divisao(&estado, &tarefa, &tx_eventos, &acordar_ui);

        let restantes = estado.workers_ocupados.fetch_sub(1, Ordering::SeqCst) - 1;
        if restantes == 0 {
            estado.condvar.notify_all();
        }
    }
}

/// Executa a divisão de uma tarefa individual reportando progresso, velocidade e ETA.
fn executar_tarefa_divisao(
    estado: &EstadoCompartilhadoFila,
    tarefa: &TarefaDivisao,
    tx_eventos: &Sender<EventoAplicacao>,
    acordar_ui: &Arc<dyn Fn() + Send + Sync>,
) {
    let inicio = Instant::now();

    let flag_cancelamento = estado
        .cancelamentos_individuais
        .lock()
        .ok()
        .and_then(|m| m.get(&tarefa.id_item).cloned())
        .unwrap_or_else(|| Arc::clone(&estado.cancelamento_global));

    if estado.cancelamento_global.load(Ordering::SeqCst) {
        flag_cancelamento.store(true, Ordering::SeqCst);
    }

    let config = Configuracao {
        entrada: tarefa.caminho.clone(),
        diretorio_saida: tarefa.diretorio_saida.clone(),
        tamanho_maximo_bytes: tarefa.tamanho_maximo_bytes,
        tamanho_alvo_bytes: tarefa.tamanho_alvo_bytes,
        concorrencia: 1,
        permitir_sobrescrita: tarefa.permitir_sobrescrita,
        recursivo: false,
        caminho_ffmpeg: Some(tarefa.executor.caminho_ffmpeg().to_path_buf()),
        caminho_ffprobe: Some(tarefa.executor.caminho_ffprobe().to_path_buf()),
        verboso: false,
        silencioso: true,
    };

    let mut ultimo_envio = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut ultima_fase = EstadoItemFila::AguardandoProcessamento;
    let tamanho_orig = tarefa.tamanho_original_bytes;

    let resultado = dividir_arquivo_video_com_observador(
        &tarefa.caminho,
        &config,
        &tarefa.executor,
        &flag_cancelamento,
        |evento| {
            let (fase, fracao, detalhe) = match evento {
                EventoProgressoDivisao::VerificandoMetadados => (
                    EstadoItemFila::Processando,
                    0.02_f32,
                    "Verificando metadados...".to_string(),
                ),
                EventoProgressoDivisao::AnalisandoPacotes => (
                    EstadoItemFila::Processando,
                    0.10_f32,
                    "Analisando keyframes e pacotes...".to_string(),
                ),
                EventoProgressoDivisao::Processando {
                    fracao,
                    tentativa,
                    segmentos_planejados,
                } => {
                    #[allow(clippy::cast_possible_truncation)]
                    let f_mapeada = 0.12_f32 + (fracao as f32).clamp(0.0, 1.0) * 0.73_f32;
                    let msg = if tentativa == 1 {
                        format!("Dividindo em ~{segmentos_planejados} partes (Stream Copy)...")
                    } else {
                        format!("Ajustando corte em keyframe anterior (tentativa {tentativa})...")
                    };
                    (EstadoItemFila::Processando, f_mapeada, msg)
                }
                EventoProgressoDivisao::Validando {
                    parte_atual,
                    total_partes,
                } => {
                    #[allow(clippy::cast_precision_loss)]
                    let frac_val = parte_atual as f32 / total_partes.max(1) as f32;
                    let f_mapeada = 0.85_f32 + frac_val * 0.14_f32;
                    (
                        EstadoItemFila::Validando,
                        f_mapeada,
                        format!("Validando parte {parte_atual}/{total_partes}..."),
                    )
                }
            };

            let mudou_fase = fase != ultima_fase;
            if !mudou_fase && ultimo_envio.elapsed() < INTERVALO_THROTTLE_PROGRESSO {
                return;
            }
            ultima_fase = fase;
            ultimo_envio = Instant::now();

            let decorrido = inicio.elapsed();
            let segs = decorrido.as_secs_f64();
            #[allow(clippy::cast_precision_loss)]
            let bytes_feitos = tamanho_orig as f64 * f64::from(fracao);
            let velocidade_mb_s = if segs > 0.05 {
                (bytes_feitos / 1_000_000.0) / segs
            } else {
                0.0
            };
            let eta_estimado = if fracao > 0.05 && fracao < 0.995 && segs > 0.1 {
                let restante = segs * (1.0 - f64::from(fracao)) / f64::from(fracao);
                if restante.is_finite() && restante >= 0.0 {
                    Some(Duration::from_secs_f64(restante))
                } else {
                    None
                }
            } else {
                None
            };

            enviar_evento(
                tx_eventos,
                acordar_ui,
                EventoAplicacao::ProgressoItem {
                    id_item: tarefa.id_item,
                    estado: fase,
                    fracao,
                    velocidade_mb_s,
                    tempo_decorrido: decorrido,
                    eta_estimado,
                    detalhe,
                },
            );
        },
    );

    let tempo_decorrido = inicio.elapsed();
    let segs = tempo_decorrido.as_secs_f64();
    #[allow(clippy::cast_precision_loss)]
    let velocidade_final = if segs > 0.001 {
        (tamanho_orig as f64 / 1_000_000.0) / segs
    } else {
        0.0
    };

    let resultado_final = match resultado {
        Ok(relatorio) => match relatorio.status {
            StatusProcessamentoVideo::Dividido {
                diretorio_destino,
                segmentos,
                ..
            } => {
                let lista_segs: Vec<InfoSegmentoGerado> = segmentos
                    .into_iter()
                    .map(|s| {
                        let nome_arquivo = s.caminho.file_name().map_or_else(
                            || s.caminho.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        InfoSegmentoGerado {
                            indice: s.indice,
                            nome_arquivo,
                            caminho: s.caminho,
                            tamanho_bytes: s.tamanho_bytes,
                            duracao_segundos: s.duracao_segundos,
                        }
                    })
                    .collect();

                if let Ok(mut stats) = estado.estatisticas.lock() {
                    stats.videos_divididos += 1;
                    stats.segmentos_gerados += lista_segs.len();
                    stats.bytes_processados = stats.bytes_processados.saturating_add(tamanho_orig);
                }

                ResultadoFinalItem {
                    estado: EstadoItemFila::Concluido,
                    diretorio_saida: Some(diretorio_destino),
                    segmentos: lista_segs,
                    tempo_decorrido,
                    velocidade_mb_s: velocidade_final,
                    mensagem_erro: None,
                    limite_utilizado_bytes: tarefa.tamanho_maximo_bytes,
                }
            }
            StatusProcessamentoVideo::IgnoradoAbaixoDoLimite { .. } => {
                if let Ok(mut stats) = estado.estatisticas.lock() {
                    stats.videos_ignorados += 1;
                    stats.bytes_processados = stats.bytes_processados.saturating_add(tamanho_orig);
                }

                ResultadoFinalItem {
                    estado: EstadoItemFila::Ignorado,
                    diretorio_saida: None,
                    segmentos: Vec::new(),
                    tempo_decorrido,
                    velocidade_mb_s: velocidade_final,
                    mensagem_erro: None,
                    limite_utilizado_bytes: tarefa.tamanho_maximo_bytes,
                }
            }
        },
        Err(ErroDivisor::CanceladoPeloUsuario) => {
            if let Ok(mut stats) = estado.estatisticas.lock() {
                stats.cancelados += 1;
            }
            ResultadoFinalItem {
                estado: EstadoItemFila::Cancelado,
                diretorio_saida: None,
                segmentos: Vec::new(),
                tempo_decorrido,
                velocidade_mb_s: 0.0,
                mensagem_erro: None,
                limite_utilizado_bytes: tarefa.tamanho_maximo_bytes,
            }
        }
        Err(erro) => {
            if matches!(erro, ErroDivisor::EspacoEmDiscoInsuficiente { .. }) {
                enviar_evento(
                    tx_eventos,
                    acordar_ui,
                    EventoAplicacao::AlertaEspacoDisco {
                        id_item: tarefa.id_item,
                        detalhe: erro.to_string(),
                    },
                );
            }
            warn!(arquivo = %tarefa.caminho.display(), %erro, "Erro ao processar item da fila");
            if let Ok(mut stats) = estado.estatisticas.lock() {
                stats.falhas += 1;
            }
            ResultadoFinalItem {
                estado: EstadoItemFila::Erro,
                diretorio_saida: None,
                segmentos: Vec::new(),
                tempo_decorrido,
                velocidade_mb_s: 0.0,
                mensagem_erro: Some(erro.to_string()),
                limite_utilizado_bytes: tarefa.tamanho_maximo_bytes,
            }
        }
    };

    enviar_evento(
        tx_eventos,
        acordar_ui,
        EventoAplicacao::ItemFinalizado {
            id_item: tarefa.id_item,
            resultado: resultado_final,
        },
    );
}

/// Envia um evento ao canal e acorda imediatamente a thread da interface.
fn enviar_evento(
    tx: &Sender<EventoAplicacao>,
    acordar_ui: &Arc<dyn Fn() + Send + Sync>,
    evento: EventoAplicacao,
) {
    if tx.send(evento).is_ok() {
        acordar_ui();
    }
}
