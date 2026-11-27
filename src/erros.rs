//! Definição estruturada dos erros do divisor de vídeo.

use std::path::PathBuf;
use thiserror::Error;

/// Tipo de resultado padrão utilizado em toda a aplicação.
pub type Resultado<T> = std::result::Result<T, ErroDivisor>;

/// Enumeração de erros possíveis durante a análise, planejamento, divisão e validação de vídeos.
#[derive(Debug, Error)]
pub enum ErroDivisor {
    /// Executável obrigatório (`ffmpeg` ou `ffprobe`) não foi encontrado no sistema.
    #[error(
        "Ferramenta multimídia obrigatória não encontrada: '{ferramenta}'. Verifique se está instalada e disponível no PATH do sistema."
    )]
    FerramentaNaoEncontrada {
        /// Nome ou caminho da ferramenta ausente.
        ferramenta: String,
    },

    /// Caminho de entrada não existe no sistema de arquivos.
    #[error("Caminho de entrada não encontrado: '{}'", caminho.display())]
    ArquivoNaoEncontrado {
        /// Caminho informado.
        caminho: PathBuf,
    },

    /// Configuração informada via CLI ou API é inválida.
    #[error("Configuração inválida: {detalhe}")]
    ConfiguracaoInvalida {
        /// Explicação detalhada da inconsistência.
        detalhe: String,
    },

    /// Arquivo de entrada está corrompido, truncado ou ilegível pelo FFprobe.
    #[error("Arquivo de vídeo corrompido ou inválido '{}': {detalhe}", caminho.display())]
    ArquivoCorrompido {
        /// Caminho do arquivo analisado.
        caminho: PathBuf,
        /// Causa reportada pelo analisador/FFprobe.
        detalhe: String,
    },

    /// O arquivo não possui nenhum fluxo de vídeo decodificável.
    #[error("Nenhum fluxo de vídeo encontrado no arquivo '{}'", caminho.display())]
    SemStreamVideo {
        /// Caminho do arquivo analisado.
        caminho: PathBuf,
    },

    /// Não foi possível identificar keyframes utilizáveis no fluxo de vídeo.
    #[error(
        "Não foram encontrados keyframes válidos no arquivo '{}': {detalhe}",
        caminho.display()
    )]
    SemKeyframes {
        /// Caminho do arquivo analisado.
        caminho: PathBuf,
        /// Detalhe adicional.
        detalhe: String,
    },

    /// A divisão respeitando o limite máximo é matematicamente impossível sem recodificar.
    #[error(
        "Impossível dividir '{}' sem recodificação: {motivo} (tamanho mínimo do bloco: {tamanho_minimo_bytes} bytes; limite máximo configurado: {limite_maximo_bytes} bytes). Nenhuma recodificação automática foi aplicada para preservar 100% da qualidade original.",
        caminho.display()
    )]
    DivisaoImpossivelSemRecodificacao {
        /// Caminho do vídeo original.
        caminho: PathBuf,
        /// Motivo claro detalhando o intervalo de keyframes problemático.
        motivo: String,
        /// Menor tamanho possível encontrado para o GOP problemático.
        tamanho_minimo_bytes: u64,
        /// Limite máximo configurado.
        limite_maximo_bytes: u64,
    },

    /// Um segmento gerado ultrapassou o limite máximo permitido.
    #[error(
        "Segmento {indice} ultrapassou o limite máximo permitido: {tamanho_bytes} bytes > {limite_bytes} bytes."
    )]
    SegmentoExcedeuLimite {
        /// Índice (base 1) do segmento.
        indice: usize,
        /// Tamanho real medido em bytes.
        tamanho_bytes: u64,
        /// Limite máximo em bytes.
        limite_bytes: u64,
    },

    /// Falha durante a validação de integridade, codecs ou continuidade dos segmentos.
    #[error("Falha na validação de integridade: {detalhe}")]
    FalhaValidacao {
        /// Descrição do problema detectado.
        detalhe: String,
    },

    /// Falha ao executar o processo `ffprobe`.
    #[error("Erro na execução do FFprobe: {detalhe}")]
    FalhaExecucaoFfprobe {
        /// Mensagem de diagnóstico ou stderr.
        detalhe: String,
    },

    /// Falha ao executar o processo `ffmpeg`.
    #[error("Erro na execução do FFmpeg (código {codigo:?}): {stderr}")]
    FalhaExecucaoFfmpeg {
        /// Código de saída retornado pelo processo.
        codigo: Option<i32>,
        /// Saída de erro padrão do FFmpeg.
        stderr: String,
    },

    /// Arquivo de destino já existe e a sobrescrita não foi autorizada pelo usuário.
    #[error(
        "Arquivo de destino já existe e sobrescrita não foi autorizada: '{}'. Utilize '--sobrescrever' para permitir a substituição.",
        caminho.display()
    )]
    SobrescritaNaoAutorizada {
        /// Caminho existente que seria sobrescrito.
        caminho: PathBuf,
    },

    /// Operação interrompida de forma segura por solicitação do usuário (SIGINT/Ctrl+C).
    #[error("Processamento cancelado com segurança pelo usuário.")]
    CanceladoPeloUsuario,

    /// Erro ao interpretar JSON retornado pelo FFprobe.
    #[error("Falha ao interpretar metadados JSON do FFprobe: {detalhe}")]
    FalhaInterpretacaoJson {
        /// Descrição do erro de desserialização.
        detalhe: String,
    },

    /// Espaço livre em disco insuficiente no volume de destino.
    #[error(
        "Espaço em disco insuficiente em '{}': disponível {disponivel_bytes} bytes, necessário aproximadamente {necessario_bytes} bytes.",
        caminho.display()
    )]
    EspacoEmDiscoInsuficiente {
        /// Diretório verificado.
        caminho: PathBuf,
        /// Espaço livre disponível em bytes.
        disponivel_bytes: u64,
        /// Espaço mínimo estimado necessário em bytes.
        necessario_bytes: u64,
    },

    /// Falha ao operar o banco de dados SQLite local do histórico.
    #[error("Erro no histórico local (SQLite): {detalhe}")]
    ErroBancoDados {
        /// Mensagem detalhada do banco de dados.
        detalhe: String,
    },

    /// Falha ao gerar ou carregar miniatura de pré-visualização.
    #[error("Falha ao gerar miniatura para '{}': {detalhe}", caminho.display())]
    ErroMiniatura {
        /// Arquivo de vídeo relacionado.
        caminho: PathBuf,
        /// Detalhe da falha.
        detalhe: String,
    },

    /// Falha na integração com o sistema operacional (abertura de arquivos/pastas/notificações).
    #[error("Erro de integração com o sistema operacional: {detalhe}")]
    ErroSistemaOperacional {
        /// Descrição do erro.
        detalhe: String,
    },

    /// Erro de entrada/saída do sistema de arquivos.
    #[error("Erro de E/S ({contexto}): {origem}")]
    ErroIo {
        /// Operação em andamento quando o erro ocorreu.
        contexto: String,
        /// Erro subjacente do sistema operacional.
        #[source]
        origem: std::io::Error,
    },
}

impl ErroDivisor {
    /// Cria um erro de E/S com contexto descritivo.
    pub fn io(contexto: impl Into<String>, origem: std::io::Error) -> Self {
        Self::ErroIo {
            contexto: contexto.into(),
            origem,
        }
    }
}
