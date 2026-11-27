//! Configuração e interface de linha de comando (CLI) da ferramenta.

use std::path::PathBuf;

use clap::Parser;

use crate::erros::{ErroDivisor, Resultado};

/// Limite máximo padrão rigoroso por segmento: 10.000.000 bytes (10 MB decimal).
pub const TAMANHO_MAXIMO_PADRAO_BYTES: u64 = 10_000_000;

/// Tamanho-alvo padrão por segmento: 9.800.000 bytes, mantendo margem de segurança de 200 KB.
pub const TAMANHO_ALVO_PADRAO_BYTES: u64 = 9_800_000;

/// Proporção padrão entre tamanho-alvo e tamanho máximo quando um limite customizado é informado.
pub const FATOR_MARGEM_SEGURANCA_PADRAO: f64 = 0.98;

/// Limite mínimo sensato para divisão de arquivos de vídeo (64 KB).
pub const TAMANHO_MINIMO_CONFIGURAVEL_BYTES: u64 = 65_536;

/// Argumentos aceitos via linha de comando.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "divisor_video",
    version,
    about = "Divide automaticamente vídeos em segmentos de até 10 MB preservando 100% da qualidade original (Stream Copy).",
    long_about = "Ferramenta em Rust que analisa keyframes e pacotes via FFprobe e divide vídeos via FFmpeg (-c copy) sem recodificação, garantindo que nenhum arquivo gerado ultrapasse o limite configurado."
)]
pub struct ArgumentosCli {
    /// Caminho para um arquivo de vídeo ou diretório contendo múltiplos vídeos.
    #[arg(value_name = "ENTRADA")]
    pub entrada: PathBuf,

    /// Diretório raiz onde serão criadas as pastas de saída dos segmentos.
    #[arg(
        short = 'o',
        long = "saida",
        default_value = "saida",
        value_name = "DIRETORIO"
    )]
    pub diretorio_saida: PathBuf,

    /// Tamanho máximo absoluto por segmento, em bytes (padrão: 10000000 = 10 MB).
    #[arg(
        short = 'm',
        long = "tamanho-maximo",
        default_value_t = TAMANHO_MAXIMO_PADRAO_BYTES,
        value_name = "BYTES"
    )]
    pub tamanho_maximo_bytes: u64,

    /// Tamanho-alvo opcional por segmento, em bytes (padrão: 9800000 para 10 MB, ou 98% do limite).
    #[arg(short = 'a', long = "tamanho-alvo", value_name = "BYTES")]
    pub tamanho_alvo_bytes: Option<u64>,

    /// Quantidade máxima de vídeos processados simultaneamente em paralelo.
    #[arg(short = 'j', long = "concorrencia", value_name = "THREADS")]
    pub concorrencia: Option<usize>,

    /// Autoriza sobrescrever arquivos já existentes no diretório de saída.
    #[arg(long = "sobrescrever", default_value_t = false)]
    pub permitir_sobrescrita: bool,

    /// Quando a entrada for um diretório, percorre subdiretórios recursivamente.
    #[arg(short = 'r', long = "recursivo", default_value_t = false)]
    pub recursivo: bool,

    /// Caminho explícito para o executável do FFmpeg (detectado automaticamente se omitido).
    #[arg(long = "ffmpeg", value_name = "CAMINHO")]
    pub caminho_ffmpeg: Option<PathBuf>,

    /// Caminho explícito para o executável do FFprobe (detectado automaticamente se omitido).
    #[arg(long = "ffprobe", value_name = "CAMINHO")]
    pub caminho_ffprobe: Option<PathBuf>,

    /// Habilita logs detalhados de depuração.
    #[arg(short = 'v', long = "verboso", default_value_t = false)]
    pub verboso: bool,

    /// Oculta barras de progresso interativas.
    #[arg(long = "silencioso", default_value_t = false)]
    pub silencioso: bool,
}

/// Configuração validada e pronta para uso pelos módulos de processamento.
#[derive(Debug, Clone)]
pub struct Configuracao {
    /// Arquivo de vídeo ou diretório de entrada.
    pub entrada: PathBuf,
    /// Diretório raiz onde os resultados serão gravados.
    pub diretorio_saida: PathBuf,
    /// Limite máximo intransponível por arquivo gerado, em bytes.
    pub tamanho_maximo_bytes: u64,
    /// Tamanho-alvo de planejamento com margem de segurança, em bytes.
    pub tamanho_alvo_bytes: u64,
    /// Limite de concorrência para processamento paralelo de múltiplos vídeos.
    pub concorrencia: usize,
    /// Indica se arquivos existentes no destino podem ser substituídos.
    pub permitir_sobrescrita: bool,
    /// Indica se a varredura de diretório deve ser recursiva.
    pub recursivo: bool,
    /// Caminho opcional do executável `ffmpeg`.
    pub caminho_ffmpeg: Option<PathBuf>,
    /// Caminho opcional do executável `ffprobe`.
    pub caminho_ffprobe: Option<PathBuf>,
    /// Ativa logs detalhados.
    pub verboso: bool,
    /// Desativa barras de progresso visuais.
    pub silencioso: bool,
}

impl Configuracao {
    /// Constrói e valida uma `Configuracao` a partir dos argumentos da linha de comando.
    pub fn a_partir_cli(args: ArgumentosCli) -> Resultado<Self> {
        let tamanho_alvo = match args.tamanho_alvo_bytes {
            Some(alvo) => alvo,
            None => calcular_tamanho_alvo_padrao(args.tamanho_maximo_bytes),
        };

        let concorrencia = match args.concorrencia {
            Some(valor) => valor,
            None => calcular_concorrencia_padrao(),
        };

        let config = Self {
            entrada: args.entrada,
            diretorio_saida: args.diretorio_saida,
            tamanho_maximo_bytes: args.tamanho_maximo_bytes,
            tamanho_alvo_bytes: tamanho_alvo,
            concorrencia,
            permitir_sobrescrita: args.permitir_sobrescrita,
            recursivo: args.recursivo,
            caminho_ffmpeg: args.caminho_ffmpeg,
            caminho_ffprobe: args.caminho_ffprobe,
            verboso: args.verboso,
            silencioso: args.silencioso,
        };

        config.validar()?;
        Ok(config)
    }

    /// Constrói uma configuração programática (útil para integrações e testes).
    pub fn nova(
        entrada: PathBuf,
        diretorio_saida: PathBuf,
        tamanho_maximo_bytes: u64,
    ) -> Resultado<Self> {
        let tamanho_alvo_bytes = calcular_tamanho_alvo_padrao(tamanho_maximo_bytes);
        let config = Self {
            entrada,
            diretorio_saida,
            tamanho_maximo_bytes,
            tamanho_alvo_bytes,
            concorrencia: calcular_concorrencia_padrao(),
            permitir_sobrescrita: false,
            recursivo: false,
            caminho_ffmpeg: None,
            caminho_ffprobe: None,
            verboso: false,
            silencioso: true,
        };
        config.validar()?;
        Ok(config)
    }

    /// Valida todas as invariantes da configuração.
    pub fn validar(&self) -> Resultado<()> {
        if self.tamanho_maximo_bytes < TAMANHO_MINIMO_CONFIGURAVEL_BYTES {
            return Err(ErroDivisor::ConfiguracaoInvalida {
                detalhe: format!(
                    "O tamanho máximo ({} bytes) deve ser de pelo menos {} bytes.",
                    self.tamanho_maximo_bytes, TAMANHO_MINIMO_CONFIGURAVEL_BYTES
                ),
            });
        }

        if self.tamanho_alvo_bytes == 0 || self.tamanho_alvo_bytes > self.tamanho_maximo_bytes {
            return Err(ErroDivisor::ConfiguracaoInvalida {
                detalhe: format!(
                    "O tamanho-alvo ({} bytes) deve ser maior que zero e menor ou igual ao tamanho máximo ({} bytes).",
                    self.tamanho_alvo_bytes, self.tamanho_maximo_bytes
                ),
            });
        }

        if self.concorrencia == 0 {
            return Err(ErroDivisor::ConfiguracaoInvalida {
                detalhe: "A concorrência deve ser maior ou igual a 1.".to_string(),
            });
        }

        if !self.entrada.exists() {
            return Err(ErroDivisor::ArquivoNaoEncontrado {
                caminho: self.entrada.clone(),
            });
        }

        Ok(())
    }
}

/// Calcula o tamanho-alvo padrão preservando exatamente 9.800.000 bytes para o limite de 10.000.000 bytes
/// e 98% do limite para valores customizados.
pub fn calcular_tamanho_alvo_padrao(tamanho_maximo_bytes: u64) -> u64 {
    if tamanho_maximo_bytes == TAMANHO_MAXIMO_PADRAO_BYTES {
        TAMANHO_ALVO_PADRAO_BYTES
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let calculado =
            (tamanho_maximo_bytes as f64 * FATOR_MARGEM_SEGURANCA_PADRAO).floor() as u64;
        calculado.clamp(1, tamanho_maximo_bytes)
    }
}

/// Calcula uma concorrência padrão equilibrada para I/O de disco (entre 1 e 4 threads).
pub fn calcular_concorrencia_padrao() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 4))
        .unwrap_or(2)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn tamanho_alvo_padrao_exato_para_10mb() {
        assert_eq!(
            calcular_tamanho_alvo_padrao(TAMANHO_MAXIMO_PADRAO_BYTES),
            TAMANHO_ALVO_PADRAO_BYTES
        );
        assert_eq!(TAMANHO_MAXIMO_PADRAO_BYTES, 10_000_000);
        assert_eq!(TAMANHO_ALVO_PADRAO_BYTES, 9_800_000);
    }

    #[test]
    fn tamanho_alvo_padrao_proporcional_para_limite_customizado() {
        assert_eq!(calcular_tamanho_alvo_padrao(5_000_000), 4_900_000);
        assert_eq!(calcular_tamanho_alvo_padrao(20_000_000), 19_600_000);
    }

    #[test]
    fn rejeita_alvo_maior_que_maximo() {
        let dir = tempfile::tempdir().ok();
        assert!(dir.is_some());
        if let Some(d) = dir {
            let args = ArgumentosCli {
                entrada: d.path().to_path_buf(),
                diretorio_saida: d.path().join("saida"),
                tamanho_maximo_bytes: 1_000_000,
                tamanho_alvo_bytes: Some(1_100_000),
                concorrencia: Some(2),
                permitir_sobrescrita: false,
                recursivo: false,
                caminho_ffmpeg: None,
                caminho_ffprobe: None,
                verboso: false,
                silencioso: true,
            };
            let resultado = Configuracao::a_partir_cli(args);
            assert!(matches!(
                resultado,
                Err(ErroDivisor::ConfiguracaoInvalida { .. })
            ));
        }
    }

    #[test]
    fn rejeita_concorrencia_zero() {
        let dir = tempfile::tempdir().ok();
        assert!(dir.is_some());
        if let Some(d) = dir {
            let args = ArgumentosCli {
                entrada: d.path().to_path_buf(),
                diretorio_saida: d.path().join("saida"),
                tamanho_maximo_bytes: TAMANHO_MAXIMO_PADRAO_BYTES,
                tamanho_alvo_bytes: None,
                concorrencia: Some(0),
                permitir_sobrescrita: false,
                recursivo: false,
                caminho_ffmpeg: None,
                caminho_ffprobe: None,
                verboso: false,
                silencioso: true,
            };
            let resultado = Configuracao::a_partir_cli(args);
            assert!(matches!(
                resultado,
                Err(ErroDivisor::ConfiguracaoInvalida { .. })
            ));
        }
    }
}
