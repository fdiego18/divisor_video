//! Tipos de estado e eventos seguros para comunicação assíncrona entre threads
//! de análise/processamento e a interface do usuário.
//!
//! Este módulo é completamente independente de bibliotecas gráficas (`egui`/`eframe`).

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Estado detalhado de um vídeo na fila de processamento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstadoItemFila {
    /// Vídeo adicionado e aguardando extração de metadados em segundo plano.
    AguardandoAnalise,
    /// Metadados e miniatura sendo extraídos em segundo plano.
    Analisando,
    /// Vídeo analisado e pronto para iniciar a divisão.
    AguardandoProcessamento,
    /// Divisão em andamento via FFmpeg (`-c copy`).
    Processando,
    /// Segmentos gerados em validação de integridade, codecs e limite.
    Validando,
    /// Divisão finalizada e validada com sucesso.
    Concluido,
    /// Vídeo já está dentro do limite máximo configurado e foi ignorado.
    Ignorado,
    /// Processamento cancelado com segurança pelo usuário.
    Cancelado,
    /// Ocorreu erro durante análise, divisão ou validação.
    Erro,
}

impl EstadoItemFila {
    /// Rótulo amigável em português brasileiro.
    pub fn rotulo(self) -> &'static str {
        match self {
            Self::AguardandoAnalise => "Aguardando análise",
            Self::Analisando => "Analisando",
            Self::AguardandoProcessamento => "Aguardando processamento",
            Self::Processando => "Processando",
            Self::Validando => "Validando",
            Self::Concluido => "Concluído",
            Self::Ignorado => "Ignorado",
            Self::Cancelado => "Cancelado",
            Self::Erro => "Erro",
        }
    }

    /// Indica se o item está atualmente em execução ativa.
    pub fn em_andamento(self) -> bool {
        matches!(self, Self::Analisando | Self::Processando | Self::Validando)
    }

    /// Indica se o item está aguardando processamento e pode ser reordenado.
    pub fn pode_reordenar(self) -> bool {
        matches!(
            self,
            Self::AguardandoAnalise | Self::AguardandoProcessamento
        )
    }
}

/// Resumo técnico de um vídeo extraído em segundo plano via FFprobe.
#[derive(Debug, Clone, Default)]
pub struct ResumoTecnicoVideo {
    /// Tamanho real do arquivo em bytes.
    pub tamanho_bytes: u64,
    /// Duração total em segundos.
    pub duracao_segundos: f64,
    /// Largura em pixels.
    pub largura: u32,
    /// Altura em pixels.
    pub altura: u32,
    /// Taxa de quadros numérica aproximada.
    pub fps: f64,
    /// Codec do fluxo de vídeo principal.
    pub codec_video: String,
    /// Codecs das faixas de áudio encontradas (ex: `"aac"`, `"aac, opus"`).
    pub codec_audio: String,
    /// Quantidade de faixas de áudio presentes.
    pub quantidade_faixas_audio: usize,
    /// Formato/contêiner reportado.
    pub formato: String,
}

impl ResumoTecnicoVideo {
    /// Formata a resolução como `"1920x1080"` ou `"—"`.
    pub fn resolucao_formatada(&self) -> String {
        if self.largura > 0 && self.altura > 0 {
            format!("{}x{}", self.largura, self.altura)
        } else {
            "—".to_string()
        }
    }

    /// Estima a quantidade de segmentos com base no tamanho do arquivo e no limite-alvo.
    pub fn estimar_quantidade_segmentos(
        &self,
        limite_maximo_bytes: u64,
        tamanho_alvo_bytes: u64,
    ) -> usize {
        if self.tamanho_bytes == 0 || limite_maximo_bytes == 0 {
            return 0;
        }
        if self.tamanho_bytes <= limite_maximo_bytes {
            return 1;
        }
        let divisor = tamanho_alvo_bytes.max(1);
        let partes = self.tamanho_bytes.div_ceil(divisor);
        usize::try_from(partes).unwrap_or(usize::MAX)
    }
}

/// Informações de um segmento gerado após divisão validada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfoSegmentoGerado {
    /// Número sequencial (base 1).
    pub indice: usize,
    /// Nome do arquivo do segmento.
    pub nome_arquivo: String,
    /// Caminho completo no disco.
    pub caminho: PathBuf,
    /// Tamanho real em bytes.
    pub tamanho_bytes: u64,
    /// Duração real em segundos.
    pub duracao_segundos: f64,
}

/// Dados consolidados da conclusão de um item da fila.
#[derive(Debug, Clone)]
pub struct ResultadoFinalItem {
    /// Estado final (`Concluido`, `Ignorado`, `Cancelado` ou `Erro`).
    pub estado: EstadoItemFila,
    /// Diretório onde os segmentos foram publicados, quando dividido.
    pub diretorio_saida: Option<PathBuf>,
    /// Lista ordenada de segmentos gerados.
    pub segmentos: Vec<InfoSegmentoGerado>,
    /// Tempo decorrido neste vídeo.
    pub tempo_decorrido: Duration,
    /// Velocidade média em MB/s.
    pub velocidade_mb_s: f64,
    /// Mensagem descritiva de erro (quando `estado == Erro`).
    pub mensagem_erro: Option<String>,
    /// Limite máximo que foi utilizado no processamento.
    pub limite_utilizado_bytes: u64,
}

/// Resumo final ao concluir toda a fila de processamento.
#[derive(Debug, Clone, Default)]
pub struct ResumoExecucaoFila {
    /// Total de vídeos processados (divididos + ignorados).
    pub videos_processados: usize,
    /// Quantidade de vídeos efetivamente divididos.
    pub videos_divididos: usize,
    /// Quantidade de vídeos ignorados por estarem abaixo do limite.
    pub videos_ignorados: usize,
    /// Total de segmentos gerados em toda a fila.
    pub segmentos_gerados: usize,
    /// Quantidade de vídeos com falha.
    pub falhas: usize,
    /// Quantidade de vídeos cancelados.
    pub cancelados: usize,
    /// Tempo total de execução da fila.
    pub tempo_total: Duration,
    /// Velocidade média global em MB/s.
    pub velocidade_media_mb_s: f64,
}

/// Eventos assíncronos enviados pelas threads de segundo plano para a interface.
#[derive(Debug, Clone)]
pub enum EventoAplicacao {
    /// Iniciou a análise de metadados/miniatura de um vídeo.
    AnaliseIniciada { id_item: u64 },
    /// Análise técnica concluída com sucesso.
    AnaliseConcluida {
        id_item: u64,
        resumo: ResumoTecnicoVideo,
    },
    /// Falha na análise técnica (ex: formato inválido ou arquivo corrompido).
    AnaliseFalhou { id_item: u64, erro: String },
    /// Miniatura pronta no cache em disco para carregamento na UI.
    MiniaturaPronta {
        id_item: u64,
        caminho_imagem: PathBuf,
    },
    /// Atualização de fase e progresso durante o processamento de um vídeo.
    ProgressoItem {
        id_item: u64,
        estado: EstadoItemFila,
        fracao: f32,
        velocidade_mb_s: f64,
        tempo_decorrido: Duration,
        eta_estimado: Option<Duration>,
        detalhe: String,
    },
    /// Processamento de um item da fila foi concluído (sucesso, ignorado, cancelado ou erro).
    ItemFinalizado {
        id_item: u64,
        resultado: ResultadoFinalItem,
    },
    /// Alerta de espaço em disco insuficiente detectado antes ou durante o processamento.
    AlertaEspacoDisco { id_item: u64, detalhe: String },
    /// Toda a fila em execução foi finalizada.
    FilaFinalizada {
        resumo: ResumoExecucaoFila,
        cancelada: bool,
    },
}
