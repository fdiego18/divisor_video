//! Estado central do aplicativo desktop, coordenação de fila visual, cache limitado
//! de texturas de miniaturas e integração com histórico SQLite e configurações persistentes.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use tracing::warn;

use crate::configuracoes_persistentes::{ConfiguracoesPersistentes, LimitePredefinido};
use crate::eventos::{
    EstadoItemFila, EventoAplicacao, InfoSegmentoGerado, ResumoExecucaoFila, ResumoTecnicoVideo,
};
use crate::executor_ffmpeg::{DiagnosticoFerramentas, ExecutorFfmpeg};
use crate::fila_processamento::{MotorFilaProcessamento, TarefaAnalise, TarefaDivisao};
use crate::historico::{
    FiltroHistorico, NovoRegistroHistorico, RegistroHistorico, RepositorioHistorico,
};
use crate::integracao_so::{abrir_no_sistema, espaco_livre_bytes};
use crate::miniaturas::GerenciadorMiniaturas;
use crate::notificacoes::{CategoriaNotificacao, CentralNotificacoes};
use crate::processamento_lote::possui_extensao_video;

/// Teto máximo de texturas de miniaturas mantidas simultaneamente em memória RAM/GPU.
const MAX_TEXTURAS_MINIATURAS_EM_MEMORIA: usize = 64;

/// Áreas principais de navegação da interface desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbaAtiva {
    Inicio,
    Historico,
    Configuracoes,
    SobreAjuda,
}

impl AbaAtiva {
    /// Nome exibido no menu lateral em português brasileiro.
    pub fn rotulo(self) -> &'static str {
        match self {
            Self::Inicio => "Início",
            Self::Historico => "Histórico",
            Self::Configuracoes => "Configurações",
            Self::SobreAjuda => "Sobre e Ajuda",
        }
    }
}

/// Representação visual de um vídeo adicionado à fila de processamento.
#[derive(Debug, Clone)]
pub struct ItemFilaGui {
    pub id: u64,
    pub caminho: PathBuf,
    pub caminho_canonico: PathBuf,
    pub nome_arquivo: String,
    pub estado: EstadoItemFila,
    pub resumo_tecnico: Option<ResumoTecnicoVideo>,
    pub caminho_miniatura: Option<PathBuf>,
    pub progresso: f32,
    pub velocidade_mb_s: f64,
    pub tempo_decorrido: Duration,
    pub eta_estimado: Option<Duration>,
    pub detalhe_progresso: String,
    pub diretorio_saida: Option<PathBuf>,
    pub segmentos: Vec<InfoSegmentoGerado>,
    pub mensagem_erro: Option<String>,
    pub expandido: bool,
    pub conflito_saida_existente: bool,
}

impl ItemFilaGui {
    /// Retorna o tamanho do arquivo em bytes (dos metadados analisados ou direto do disco).
    pub fn tamanho_bytes(&self) -> u64 {
        self.resumo_tecnico
            .as_ref()
            .map(|r| r.tamanho_bytes)
            .or_else(|| fs::metadata(&self.caminho).ok().map(|m| m.len()))
            .unwrap_or(0)
    }

    /// Verifica se já existem partes geradas previamente para este vídeo no `diretorio_saida`.
    pub fn atualizar_deteccao_conflito(&mut self, diretorio_saida: &Path) {
        let nome_base = self
            .caminho
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "video".to_string());
        let extensao = self
            .caminho
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|| "mp4".to_string());

        let pasta_destino = diretorio_saida.join(&nome_base);
        if !pasta_destino.is_dir() {
            self.conflito_saida_existente = false;
            return;
        }

        let prefixo = format!("{nome_base}_parte_");
        let sufixo = format!(".{extensao}");
        self.conflito_saida_existente = fs::read_dir(&pasta_destino).ok().is_some_and(|entradas| {
            entradas.flatten().any(|e| {
                let nome = e.file_name().to_string_lossy().into_owned();
                nome.starts_with(&prefixo) && nome.to_ascii_lowercase().ends_with(&sufixo)
            })
        });
    }
}

/// Estado global completo do aplicativo desktop.
pub struct EstadoAplicacao {
    pub aba_ativa: AbaAtiva,
    pub configuracoes: ConfiguracoesPersistentes,
    pub campo_limite_personalizado_mb: String,
    pub diagnostico: DiagnosticoFerramentas,
    pub executor: Option<ExecutorFfmpeg>,
    pub miniaturas: GerenciadorMiniaturas,
    pub motor: MotorFilaProcessamento,
    pub notificacoes: CentralNotificacoes,

    // Fila de vídeos
    pub proximo_id_item: u64,
    pub itens_fila: Vec<ItemFilaGui>,
    pub inicio_fila_atual: Option<Instant>,
    pub ultimo_resumo_fila: Option<ResumoExecucaoFila>,
    pub espaco_livre_saida_bytes: Option<u64>,
    pub ultima_checagem_espaco: Instant,

    // Cache limitado de texturas na GPU/RAM
    pub texturas_miniaturas: HashMap<u64, TextureHandle>,
    pub ordem_texturas: VecDeque<u64>,

    // Histórico SQLite
    pub repositorio_historico: Option<RepositorioHistorico>,
    pub filtro_historico: FiltroHistorico,
    pub registros_historico: Vec<RegistroHistorico>,
    pub historico_expandido: HashSet<i64>,
    pub confirmar_limpeza_historico: bool,
    pub erro_historico: Option<String>,
}

impl EstadoAplicacao {
    /// Inicializa todo o estado do aplicativo desktop carregando configurações, diagnóstico e histórico.
    pub fn novo(acordar_ui: Arc<dyn Fn() + Send + Sync>) -> Self {
        let configuracoes = ConfiguracoesPersistentes::carregar_padrao();
        #[allow(clippy::cast_precision_loss)]
        let limite_mb = configuracoes.limite_personalizado_bytes as f64 / 1_000_000.0;
        let campo_limite_personalizado_mb = if (limite_mb.fract()).abs() < 0.001 {
            format!("{limite_mb:.0}")
        } else {
            format!("{limite_mb:.2}")
        };

        let diagnostico = ExecutorFfmpeg::diagnosticar(
            configuracoes.caminho_ffmpeg.as_deref(),
            configuracoes.caminho_ffprobe.as_deref(),
        );
        let executor = ExecutorFfmpeg::detectar(
            configuracoes.caminho_ffmpeg.clone(),
            configuracoes.caminho_ffprobe.clone(),
        )
        .ok();

        let mut notificacoes = CentralNotificacoes::nova();
        if !diagnostico.tudo_disponivel() {
            notificacoes.emitir(
                CategoriaNotificacao::DependenciasIndisponiveis,
                "FFmpeg / FFprobe indisponíveis",
                "Instale FFmpeg e FFprobe ou informe os caminhos em Configurações.",
                &configuracoes.notificacoes,
            );
        }

        let (repositorio_historico, erro_historico) = match RepositorioHistorico::abrir_padrao() {
            Ok(repo) => (Some(repo), None),
            Err(erro) => {
                warn!(%erro, "Não foi possível abrir o banco SQLite de histórico");
                (None, Some(erro.to_string()))
            }
        };

        let espaco_livre_saida_bytes = espaco_livre_bytes(&configuracoes.diretorio_saida).ok();
        let motor = MotorFilaProcessamento::novo(acordar_ui);

        let mut estado = Self {
            aba_ativa: AbaAtiva::Inicio,
            configuracoes,
            campo_limite_personalizado_mb,
            diagnostico,
            executor,
            miniaturas: GerenciadorMiniaturas::padrao(),
            motor,
            notificacoes,
            proximo_id_item: 1,
            itens_fila: Vec::new(),
            inicio_fila_atual: None,
            ultimo_resumo_fila: None,
            espaco_livre_saida_bytes,
            ultima_checagem_espaco: Instant::now(),
            texturas_miniaturas: HashMap::new(),
            ordem_texturas: VecDeque::new(),
            repositorio_historico,
            filtro_historico: FiltroHistorico::default(),
            registros_historico: Vec::new(),
            historico_expandido: HashSet::new(),
            confirmar_limpeza_historico: false,
            erro_historico,
        };

        estado.recarregar_historico();
        estado
    }

    /// Reexecuta o diagnóstico de detecção de FFmpeg e FFprobe (ordem 1 -> 4).
    pub fn atualizar_diagnostico_ferramentas(&mut self) {
        self.diagnostico = ExecutorFfmpeg::diagnosticar(
            self.configuracoes.caminho_ffmpeg.as_deref(),
            self.configuracoes.caminho_ffprobe.as_deref(),
        );
        self.executor = ExecutorFfmpeg::detectar(
            self.configuracoes.caminho_ffmpeg.clone(),
            self.configuracoes.caminho_ffprobe.clone(),
        )
        .ok();

        if !self.diagnostico.tudo_disponivel() {
            self.notificacoes.emitir(
                CategoriaNotificacao::DependenciasIndisponiveis,
                "Dependências indisponíveis",
                "FFmpeg e/ou FFprobe não foram encontrados. Verifique os caminhos.",
                &self.configuracoes.notificacoes,
            );
        }
    }

    /// Persiste as configurações atuais em disco de forma atômica.
    pub fn salvar_configuracoes(&mut self) {
        if let Err(erro) = self.configuracoes.salvar_padrao() {
            warn!(%erro, "Falha ao salvar configurações persistentes");
        }
        self.espaco_livre_saida_bytes =
            espaco_livre_bytes(&self.configuracoes.diretorio_saida).ok();
        self.ultima_checagem_espaco = Instant::now();
        self.atualizar_conflitos_fila();
    }

    /// Recalcula avisos de arquivos de saída existentes para todos os itens da fila.
    pub fn atualizar_conflitos_fila(&mut self) {
        let dir = self.configuracoes.diretorio_saida.clone();
        for item in &mut self.itens_fila {
            item.atualizar_deteccao_conflito(&dir);
        }
    }

    /// Aplica o texto digitado pelo usuário no campo de limite personalizado (em MB decimal).
    pub fn aplicar_limite_personalizado(&mut self) {
        let normalizado = self.campo_limite_personalizado_mb.trim().replace(',', ".");
        if let Ok(mb) = normalizado.parse::<f64>()
            && mb.is_finite()
            && (0.5..=100_000.0).contains(&mb)
        {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let bytes = (mb * 1_000_000.0).round() as u64;
            self.configuracoes.limite_predefinido = LimitePredefinido::Personalizado;
            self.configuracoes.limite_personalizado_bytes = bytes;
            self.salvar_configuracoes();
        }
    }

    /// Adiciona arquivos e/ou diretórios à fila, detectando duplicados e formatos inválidos
    /// e disparando análise técnica + miniatura em segundo plano.
    pub fn adicionar_caminhos(&mut self, caminhos: Vec<PathBuf>, recursivo: bool) {
        let mut candidatos_video = Vec::new();
        let mut invalidos = 0usize;

        for caminho in caminhos {
            if caminho.is_dir() {
                coletar_videos_para_gui(&caminho, recursivo, &mut candidatos_video);
            } else if caminho.is_file() {
                if possui_extensao_video(&caminho) {
                    candidatos_video.push(caminho);
                } else {
                    invalidos += 1;
                }
            }
        }

        candidatos_video.sort();
        let existentes: HashSet<PathBuf> = self
            .itens_fila
            .iter()
            .map(|i| i.caminho_canonico.clone())
            .collect();

        let mut adicionados = 0usize;
        let mut duplicados = 0usize;
        let mut vistos_neste_lote: HashSet<PathBuf> = HashSet::new();

        for caminho in candidatos_video {
            let canonico = fs::canonicalize(&caminho).unwrap_or_else(|_| caminho.clone());
            if existentes.contains(&canonico) || !vistos_neste_lote.insert(canonico.clone()) {
                duplicados += 1;
                continue;
            }

            let nome_arquivo = caminho
                .file_name()
                .map_or_else(|| "video".to_string(), |n| n.to_string_lossy().into_owned());

            let id = self.proximo_id_item;
            self.proximo_id_item = self.proximo_id_item.wrapping_add(1);

            let mut item = ItemFilaGui {
                id,
                caminho: caminho.clone(),
                caminho_canonico: canonico,
                nome_arquivo,
                estado: EstadoItemFila::AguardandoAnalise,
                resumo_tecnico: None,
                caminho_miniatura: None,
                progresso: 0.0,
                velocidade_mb_s: 0.0,
                tempo_decorrido: Duration::ZERO,
                eta_estimado: None,
                detalhe_progresso: String::new(),
                diretorio_saida: None,
                segmentos: Vec::new(),
                mensagem_erro: None,
                expandido: false,
                conflito_saida_existente: false,
            };
            item.atualizar_deteccao_conflito(&self.configuracoes.diretorio_saida);

            if let Some(executor) = self.executor.clone() {
                self.motor.solicitar_analise(TarefaAnalise {
                    id_item: id,
                    caminho,
                    executor,
                    miniaturas: self.miniaturas.clone(),
                });
            } else {
                item.estado = EstadoItemFila::Erro;
                item.mensagem_erro = Some(
                    "FFmpeg/FFprobe não encontrados. Configure os executáveis na aba Configurações."
                        .to_string(),
                );
            }

            self.itens_fila.push(item);
            adicionados += 1;
        }

        if duplicados > 0 || invalidos > 0 {
            let mut partes = Vec::new();
            if adicionados > 0 {
                partes.push(format!("{adicionados} vídeo(s) adicionado(s)"));
            }
            if duplicados > 0 {
                partes.push(format!("{duplicados} duplicado(s) ignorado(s)"));
            }
            if invalidos > 0 {
                partes.push(format!("{invalidos} formato(s) inválido(s) ignorado(s)"));
            }
            self.notificacoes.emitir(
                CategoriaNotificacao::Informativa,
                "Seleção de arquivos",
                partes.join(" • "),
                &self.configuracoes.notificacoes,
            );
        }
    }

    /// Remove um vídeo individual da fila (somente se não estiver em processamento ativo).
    pub fn remover_item(&mut self, id_item: u64) {
        if let Some(pos) = self.itens_fila.iter().position(|i| i.id == id_item)
            && !self.itens_fila[pos].estado.em_andamento()
        {
            self.itens_fila.remove(pos);
            self.texturas_miniaturas.remove(&id_item);
            self.ordem_texturas.retain(|id| *id != id_item);
        }
    }

    /// Remove todos os itens da fila que não estão em processamento ativo.
    pub fn limpar_fila(&mut self) {
        let manter_ids: HashSet<u64> = self
            .itens_fila
            .iter()
            .filter(|i| i.estado.em_andamento())
            .map(|i| i.id)
            .collect();

        self.itens_fila.retain(|i| manter_ids.contains(&i.id));
        self.texturas_miniaturas
            .retain(|id, _| manter_ids.contains(id));
        self.ordem_texturas.retain(|id| manter_ids.contains(id));

        if self.itens_fila.is_empty() {
            self.ultimo_resumo_fila = None;
        }
    }

    /// Move um item aguardando processamento uma posição para cima na fila.
    pub fn mover_item_para_cima(&mut self, id_item: u64) {
        if let Some(pos) = self.itens_fila.iter().position(|i| i.id == id_item)
            && pos > 0
            && self.itens_fila[pos].estado.pode_reordenar()
            && self.itens_fila[pos - 1].estado.pode_reordenar()
        {
            self.itens_fila.swap(pos, pos - 1);
        }
    }

    /// Move um item aguardando processamento uma posição para baixo na fila.
    pub fn mover_item_para_baixo(&mut self, id_item: u64) {
        if let Some(pos) = self.itens_fila.iter().position(|i| i.id == id_item)
            && pos + 1 < self.itens_fila.len()
            && self.itens_fila[pos].estado.pode_reordenar()
            && self.itens_fila[pos + 1].estado.pode_reordenar()
        {
            self.itens_fila.swap(pos, pos + 1);
        }
    }

    /// Inicia o processamento (ou enfileira novos itens caso um processamento já esteja em andamento).
    pub fn iniciar_ou_enfileirar_pendentes(&mut self) {
        let Some(executor) = self.executor.clone() else {
            self.notificacoes.emitir(
                CategoriaNotificacao::DependenciasIndisponiveis,
                "Não foi possível iniciar",
                "FFmpeg e FFprobe são necessários para processar vídeos.",
                &self.configuracoes.notificacoes,
            );
            return;
        };

        let limite_max = self.configuracoes.limite_maximo_efetivo_bytes();
        let limite_alvo = self.configuracoes.tamanho_alvo_efetivo_bytes();
        let dir_saida = self.configuracoes.diretorio_saida.clone();
        let sobrescrever = self.configuracoes.permitir_sobrescrita;

        let mut tarefas = Vec::new();
        for item in &mut self.itens_fila {
            if matches!(
                item.estado,
                EstadoItemFila::AguardandoProcessamento | EstadoItemFila::AguardandoAnalise
            ) {
                item.progresso = 0.0;
                item.mensagem_erro = None;
                item.segmentos.clear();
                item.detalhe_progresso = "Na fila de execução...".to_string();
                tarefas.push(TarefaDivisao {
                    id_item: item.id,
                    caminho: item.caminho.clone(),
                    tamanho_original_bytes: item.tamanho_bytes(),
                    diretorio_saida: dir_saida.clone(),
                    tamanho_maximo_bytes: limite_max,
                    tamanho_alvo_bytes: limite_alvo,
                    permitir_sobrescrita: sobrescrever,
                    executor: executor.clone(),
                });
            }
        }

        if tarefas.is_empty() {
            return;
        }

        if !self.motor.esta_processando() {
            self.inicio_fila_atual = Some(Instant::now());
            self.ultimo_resumo_fila = None;
        }

        self.motor
            .iniciar_ou_enfileirar(tarefas, self.configuracoes.concorrencia);
    }

    /// Reenfileira todos os itens com falha ou cancelados para novo processamento.
    pub fn reprocessar_falhas(&mut self) {
        let mut alterados = 0usize;
        for item in &mut self.itens_fila {
            if matches!(
                item.estado,
                EstadoItemFila::Erro | EstadoItemFila::Cancelado
            ) {
                item.estado = EstadoItemFila::AguardandoProcessamento;
                item.mensagem_erro = None;
                item.progresso = 0.0;
                item.detalhe_progresso.clear();
                alterados += 1;
            }
        }
        if alterados > 0 {
            self.iniciar_ou_enfileirar_pendentes();
        }
    }

    /// Reprocessa um item específico da fila.
    pub fn reprocessar_item(&mut self, id_item: u64) {
        let Some(executor) = self.executor.clone() else {
            return;
        };
        let limite_max = self.configuracoes.limite_maximo_efetivo_bytes();
        let limite_alvo = self.configuracoes.tamanho_alvo_efetivo_bytes();
        let dir_saida = self.configuracoes.diretorio_saida.clone();
        let sobrescrever = self.configuracoes.permitir_sobrescrita;

        if let Some(item) = self.itens_fila.iter_mut().find(|i| i.id == id_item)
            && !item.estado.em_andamento()
        {
            item.estado = EstadoItemFila::AguardandoProcessamento;
            item.mensagem_erro = None;
            item.progresso = 0.0;
            item.segmentos.clear();
            let tarefa = TarefaDivisao {
                id_item: item.id,
                caminho: item.caminho.clone(),
                tamanho_original_bytes: item.tamanho_bytes(),
                diretorio_saida: dir_saida,
                tamanho_maximo_bytes: limite_max,
                tamanho_alvo_bytes: limite_alvo,
                permitir_sobrescrita: sobrescrever,
                executor,
            };
            if !self.motor.esta_processando() {
                self.inicio_fila_atual = Some(Instant::now());
            }
            self.motor
                .iniciar_ou_enfileirar(vec![tarefa], self.configuracoes.concorrencia);
        }
    }

    /// Reprocessa um vídeo originado do histórico, adicionando-o à aba Início.
    pub fn reprocessar_do_historico(&mut self, caminho: PathBuf) {
        if !caminho.is_file() {
            self.notificacoes.emitir(
                CategoriaNotificacao::ProcessamentoComFalhas,
                "Arquivo não encontrado",
                format!(
                    "O arquivo original '{}' não existe mais neste caminho.",
                    caminho.display()
                ),
                &self.configuracoes.notificacoes,
            );
            return;
        }
        self.aba_ativa = AbaAtiva::Inicio;
        let canonico = fs::canonicalize(&caminho).unwrap_or_else(|_| caminho.clone());
        if let Some(existente) = self
            .itens_fila
            .iter()
            .find(|i| i.caminho_canonico == canonico)
            .map(|i| i.id)
        {
            self.reprocessar_item(existente);
        } else {
            self.adicionar_caminhos(vec![caminho], false);
        }
    }

    /// Cancela um vídeo individualmente.
    pub fn cancelar_item(&mut self, id_item: u64) {
        self.motor.cancelar_item(id_item);
    }

    /// Cancela toda a fila em execução.
    pub fn cancelar_toda_fila(&mut self) {
        self.motor.cancelar_tudo();
        self.notificacoes.emitir(
            CategoriaNotificacao::Cancelamento,
            "Cancelamento solicitado",
            "Encerrando processos ativos e removendo arquivos temporários...",
            &self.configuracoes.notificacoes,
        );
    }

    /// Drena todos os eventos assíncronos recebidos das threads de fundo e atualiza o estado.
    pub fn processar_eventos_pendentes(&mut self, ctx: Option<&Context>) {
        self.notificacoes.limpar_expiradas();

        // Atualiza espaço livre em disco periodicamente (no máximo a cada 10 segundos).
        if self.ultima_checagem_espaco.elapsed() >= Duration::from_secs(10) {
            self.espaco_livre_saida_bytes =
                espaco_livre_bytes(&self.configuracoes.diretorio_saida).ok();
            self.ultima_checagem_espaco = Instant::now();
        }

        let eventos = self.motor.drenar_eventos();
        let mut historico_alterado = false;

        for evento in eventos {
            match evento {
                EventoAplicacao::AnaliseIniciada { id_item } => {
                    if let Some(item) = self.itens_fila.iter_mut().find(|i| i.id == id_item) {
                        item.estado = EstadoItemFila::Analisando;
                        item.detalhe_progresso = "Analisando metadados...".to_string();
                    }
                }
                EventoAplicacao::AnaliseConcluida { id_item, resumo } => {
                    if let Some(item) = self.itens_fila.iter_mut().find(|i| i.id == id_item) {
                        item.resumo_tecnico = Some(resumo);
                        if item.estado == EstadoItemFila::Analisando
                            || item.estado == EstadoItemFila::AguardandoAnalise
                        {
                            item.estado = EstadoItemFila::AguardandoProcessamento;
                            item.detalhe_progresso.clear();
                        }
                    }
                }
                EventoAplicacao::AnaliseFalhou { id_item, erro } => {
                    if let Some(item) = self.itens_fila.iter_mut().find(|i| i.id == id_item) {
                        item.estado = EstadoItemFila::Erro;
                        item.mensagem_erro = Some(erro);
                        item.detalhe_progresso.clear();
                    }
                }
                EventoAplicacao::MiniaturaPronta {
                    id_item,
                    caminho_imagem,
                } => {
                    if let Some(item) = self.itens_fila.iter_mut().find(|i| i.id == id_item) {
                        item.caminho_miniatura = Some(caminho_imagem.clone());
                        if let Some(egui_ctx) = ctx {
                            self.carregar_textura_miniatura(egui_ctx, id_item, &caminho_imagem);
                        }
                    }
                }
                EventoAplicacao::ProgressoItem {
                    id_item,
                    estado,
                    fracao,
                    velocidade_mb_s,
                    tempo_decorrido,
                    eta_estimado,
                    detalhe,
                } => {
                    if let Some(item) = self.itens_fila.iter_mut().find(|i| i.id == id_item) {
                        item.estado = estado;
                        item.progresso = fracao;
                        item.velocidade_mb_s = velocidade_mb_s;
                        item.tempo_decorrido = tempo_decorrido;
                        item.eta_estimado = eta_estimado;
                        item.detalhe_progresso = detalhe;
                    }
                }
                EventoAplicacao::AlertaEspacoDisco { detalhe, .. } => {
                    self.notificacoes.emitir(
                        CategoriaNotificacao::FaltaEspacoDisco,
                        "Espaço em disco insuficiente",
                        detalhe,
                        &self.configuracoes.notificacoes,
                    );
                }
                EventoAplicacao::ItemFinalizado { id_item, resultado } => {
                    if let Some(item) = self.itens_fila.iter_mut().find(|i| i.id == id_item) {
                        item.estado = resultado.estado;
                        item.progresso = if matches!(
                            resultado.estado,
                            EstadoItemFila::Concluido | EstadoItemFila::Ignorado
                        ) {
                            1.0
                        } else {
                            item.progresso
                        };
                        item.tempo_decorrido = resultado.tempo_decorrido;
                        item.velocidade_mb_s = resultado.velocidade_mb_s;
                        item.eta_estimado = None;
                        item.diretorio_saida = resultado.diretorio_saida.clone();
                        item.segmentos = resultado.segmentos.clone();
                        item.mensagem_erro = resultado.mensagem_erro.clone();
                        item.detalhe_progresso.clear();
                        item.atualizar_deteccao_conflito(&self.configuracoes.diretorio_saida);

                        // Registra automaticamente no banco SQLite de histórico.
                        if let Some(repo) = &self.repositorio_historico {
                            let novo = NovoRegistroHistorico {
                                nome_video: item.nome_arquivo.clone(),
                                caminho_video: item.caminho.clone(),
                                data_hora: None,
                                tamanho_original_bytes: item.tamanho_bytes(),
                                limite_configurado_bytes: resultado.limite_utilizado_bytes,
                                segmentos: resultado.segmentos.clone(),
                                diretorio_saida: resultado.diretorio_saida.clone(),
                                tempo_processamento_ms: u64::try_from(
                                    resultado.tempo_decorrido.as_millis(),
                                )
                                .unwrap_or(u64::MAX),
                                velocidade_media_mb_s: resultado.velocidade_mb_s,
                                resultado: resultado.estado.rotulo().to_string(),
                                mensagem_erro: resultado.mensagem_erro.clone(),
                            };
                            if let Err(erro) = repo.inserir(&novo) {
                                warn!(%erro, "Falha ao gravar registro no histórico SQLite");
                            } else {
                                historico_alterado = true;
                            }
                        }

                        // Notificações individuais conforme preferências.
                        let nome = item.nome_arquivo.clone();
                        match resultado.estado {
                            EstadoItemFila::Concluido => {
                                self.notificacoes.emitir(
                                    CategoriaNotificacao::ProcessamentoConcluido,
                                    "Vídeo dividido com sucesso",
                                    format!(
                                        "{nome} dividido em {} parte(s).",
                                        resultado.segmentos.len()
                                    ),
                                    &self.configuracoes.notificacoes,
                                );
                            }
                            EstadoItemFila::Erro => {
                                self.notificacoes.emitir(
                                    CategoriaNotificacao::ProcessamentoComFalhas,
                                    "Falha no processamento",
                                    format!(
                                        "{nome}: {}",
                                        resultado
                                            .mensagem_erro
                                            .as_deref()
                                            .unwrap_or("Erro desconhecido")
                                    ),
                                    &self.configuracoes.notificacoes,
                                );
                            }
                            _ => {}
                        }
                    }
                }
                EventoAplicacao::FilaFinalizada { resumo, cancelada } => {
                    self.inicio_fila_atual = None;
                    self.ultimo_resumo_fila = Some(resumo.clone());
                    self.espaco_livre_saida_bytes =
                        espaco_livre_bytes(&self.configuracoes.diretorio_saida).ok();

                    if cancelada {
                        self.notificacoes.emitir(
                            CategoriaNotificacao::Cancelamento,
                            "Fila cancelada",
                            format!(
                                "{} cancelado(s) • {} concluído(s).",
                                resumo.cancelados, resumo.videos_processados
                            ),
                            &self.configuracoes.notificacoes,
                        );
                    } else {
                        self.notificacoes.emitir(
                            CategoriaNotificacao::FilaFinalizada,
                            "Fila finalizada",
                            format!(
                                "{} vídeo(s) • {} parte(s) • {} falha(s) • {:.1} MB/s",
                                resumo.videos_processados,
                                resumo.segmentos_gerados,
                                resumo.falhas,
                                resumo.velocidade_media_mb_s
                            ),
                            &self.configuracoes.notificacoes,
                        );

                        if self.configuracoes.abrir_pasta_ao_concluir
                            && resumo.videos_divididos > 0
                            && self.configuracoes.diretorio_saida.exists()
                        {
                            let _ = abrir_no_sistema(&self.configuracoes.diretorio_saida);
                        }
                    }
                }
            }
        }

        if historico_alterado {
            self.recarregar_historico();
        }
    }

    /// Carrega uma miniatura do disco para a GPU respeitando o teto de memória `MAX_TEXTURAS_MINIATURAS_EM_MEMORIA`.
    fn carregar_textura_miniatura(&mut self, ctx: &Context, id_item: u64, caminho: &Path) {
        if let Ok(rgba) = GerenciadorMiniaturas::carregar_rgba(caminho) {
            let imagem =
                ColorImage::from_rgba_unmultiplied([rgba.largura, rgba.altura], &rgba.pixels);
            let textura = ctx.load_texture(
                format!("miniatura_{id_item}"),
                imagem,
                TextureOptions::LINEAR,
            );
            self.texturas_miniaturas.insert(id_item, textura);
            self.ordem_texturas.retain(|id| *id != id_item);
            self.ordem_texturas.push_back(id_item);

            while self.ordem_texturas.len() > MAX_TEXTURAS_MINIATURAS_EM_MEMORIA {
                if let Some(mais_antiga) = self.ordem_texturas.pop_front() {
                    self.texturas_miniaturas.remove(&mais_antiga);
                }
            }
        }
    }

    /// Recarrega a lista de registros do banco SQLite usando o filtro atual.
    pub fn recarregar_historico(&mut self) {
        if let Some(repo) = &self.repositorio_historico {
            match repo.consultar(&self.filtro_historico) {
                Ok(lista) => {
                    self.registros_historico = lista;
                    self.erro_historico = None;
                }
                Err(erro) => {
                    self.erro_historico = Some(erro.to_string());
                }
            }
        }
    }

    /// Calcula o progresso global da fila (`0.0..=1.0`) ponderado pelo tamanho dos arquivos.
    pub fn progresso_global(&self) -> f32 {
        if self.itens_fila.is_empty() {
            return 0.0;
        }
        let mut peso_total = 0.0_f64;
        let mut concluido = 0.0_f64;
        for item in &self.itens_fila {
            #[allow(clippy::cast_precision_loss)]
            let peso = item.tamanho_bytes().max(1) as f64;
            let frac = match item.estado {
                EstadoItemFila::Concluido | EstadoItemFila::Ignorado => 1.0,
                EstadoItemFila::Processando | EstadoItemFila::Validando => {
                    f64::from(item.progresso.clamp(0.0, 1.0))
                }
                _ => 0.0,
            };
            peso_total += peso;
            concluido += peso * frac;
        }
        if peso_total <= 0.0 {
            0.0
        } else {
            #[allow(clippy::cast_possible_truncation)]
            let res = (concluido / peso_total).clamp(0.0, 1.0) as f32;
            res
        }
    }

    /// Quantidade de itens pendentes prontos para iniciar ou enfileirar.
    pub fn quantidade_prontos(&self) -> usize {
        self.itens_fila
            .iter()
            .filter(|i| {
                matches!(
                    i.estado,
                    EstadoItemFila::AguardandoProcessamento | EstadoItemFila::AguardandoAnalise
                )
            })
            .count()
    }

    /// Quantidade de itens com falha ou cancelados que podem ser reprocessados.
    pub fn quantidade_reprocessaveis(&self) -> usize {
        self.itens_fila
            .iter()
            .filter(|i| matches!(i.estado, EstadoItemFila::Erro | EstadoItemFila::Cancelado))
            .count()
    }
}

/// Coleta arquivos de vídeo de um diretório (opcionalmente recursivo) para adição na fila GUI.
fn coletar_videos_para_gui(dir: &Path, recursivo: bool, saida: &mut Vec<PathBuf>) {
    let Ok(entradas) = fs::read_dir(dir) else {
        return;
    };
    for entrada_res in entradas.flatten() {
        let caminho = entrada_res.path();
        if caminho.is_dir() {
            if recursivo
                && !caminho
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(".divisor_tmp_"))
            {
                coletar_videos_para_gui(&caminho, true, saida);
            }
        } else if caminho.is_file() && possui_extensao_video(&caminho) {
            saida.push(caminho);
        }
    }
}
