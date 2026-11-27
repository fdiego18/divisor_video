//! Persistência local de configurações do aplicativo desktop e limites de tamanho.
//!
//! Salva preferências no diretório padrão de configurações do sistema operacional
//! utilizando escrita atômica (arquivo temporário + renomeação) para evitar corrupção.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::configuracao::{
    TAMANHO_ALVO_PADRAO_BYTES, TAMANHO_MAXIMO_PADRAO_BYTES, calcular_concorrencia_padrao,
};
use crate::erros::{ErroDivisor, Resultado};
use crate::integracao_so::{diretorio_configuracoes, diretorio_saida_padrao, garantir_diretorio};

/// Nome do arquivo JSON de configurações persistentes.
const NOME_ARQUIVO_CONFIGURACOES: &str = "configuracoes.json";

/// Tema visual selecionado pelo usuário.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TemaPreferido {
    /// Detecta automaticamente o tema claro/escuro do sistema operacional.
    #[default]
    Sistema,
    /// Força tema escuro.
    Escuro,
    /// Força tema claro.
    Claro,
}

impl TemaPreferido {
    /// Rótulo em português brasileiro.
    pub fn rotulo(self) -> &'static str {
        match self {
            Self::Sistema => "Sistema (automático)",
            Self::Escuro => "Escuro",
            Self::Claro => "Claro",
        }
    }

    /// Lista completa de opções disponíveis.
    pub const TODAS: &'static [Self] = &[Self::Sistema, Self::Escuro, Self::Claro];
}

/// Opções predefinidas de limite máximo por segmento (unidades decimais: 1 MB = 1.000.000 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LimitePredefinido {
    /// 5 MB = 5.000.000 bytes.
    Mb5,
    /// 10 MB = 10.000.000 bytes (padrão).
    #[default]
    Mb10,
    /// 20 MB = 20.000.000 bytes.
    Mb20,
    /// 25 MB = 25.000.000 bytes.
    Mb25,
    /// 50 MB = 50.000.000 bytes.
    Mb50,
    /// 100 MB = 100.000.000 bytes.
    Mb100,
    /// 200 MB = 200.000.000 bytes.
    Mb200,
    /// 500 MB = 500.000.000 bytes.
    Mb500,
    /// 1 GB = 1.000.000.000 bytes.
    Gb1,
    /// Valor personalizado configurado pelo usuário.
    Personalizado,
}

impl LimitePredefinido {
    /// Lista ordenada de todas as opções exibidas na interface.
    pub const TODAS: &'static [Self] = &[
        Self::Mb5,
        Self::Mb10,
        Self::Mb20,
        Self::Mb25,
        Self::Mb50,
        Self::Mb100,
        Self::Mb200,
        Self::Mb500,
        Self::Gb1,
        Self::Personalizado,
    ];

    /// Rótulo amigável em português brasileiro.
    pub fn rotulo(self) -> &'static str {
        match self {
            Self::Mb5 => "5 MB",
            Self::Mb10 => "10 MB (padrão)",
            Self::Mb20 => "20 MB",
            Self::Mb25 => "25 MB",
            Self::Mb50 => "50 MB",
            Self::Mb100 => "100 MB",
            Self::Mb200 => "200 MB",
            Self::Mb500 => "500 MB",
            Self::Gb1 => "1 GB",
            Self::Personalizado => "Personalizado",
        }
    }

    /// Valor em bytes decimais (`Some(bytes)`) para opções fixas, ou `None` para `Personalizado`.
    pub fn bytes_fixos(self) -> Option<u64> {
        match self {
            Self::Mb5 => Some(5_000_000),
            Self::Mb10 => Some(10_000_000),
            Self::Mb20 => Some(20_000_000),
            Self::Mb25 => Some(25_000_000),
            Self::Mb50 => Some(50_000_000),
            Self::Mb100 => Some(100_000_000),
            Self::Mb200 => Some(200_000_000),
            Self::Mb500 => Some(500_000_000),
            Self::Gb1 => Some(1_000_000_000),
            Self::Personalizado => None,
        }
    }
}

/// Calcula automaticamente o tamanho-alvo (com margem de segurança) de acordo com o limite máximo
/// selecionado em unidades decimais (`1 MB = 1.000.000 bytes`).
///
/// - Para `10.000.000` bytes (10 MB padrão): retorna exatamente `9.800.000` bytes (margem 200 KB).
/// - Para limites até 10 MB: aplica margem de 2%.
/// - Para limites entre 10 MB e 100 MB: aplica margem de 1,5% (mínimo 200 KB).
/// - Para limites acima de 100 MB: aplica margem de 1% limitada a 4 MB, evitando desperdício.
pub fn calcular_tamanho_alvo_adaptativo(limite_maximo_bytes: u64) -> u64 {
    if limite_maximo_bytes == TAMANHO_MAXIMO_PADRAO_BYTES {
        return TAMANHO_ALVO_PADRAO_BYTES;
    }

    let margem = if limite_maximo_bytes <= 10_000_000 {
        (limite_maximo_bytes / 50).max(16_384)
    } else if limite_maximo_bytes <= 100_000_000 {
        ((limite_maximo_bytes * 15) / 1_000).max(200_000)
    } else {
        (limite_maximo_bytes / 100).clamp(1_500_000, 4_000_000)
    };

    limite_maximo_bytes
        .saturating_sub(margem)
        .max(1)
        .min(limite_maximo_bytes)
}

/// Preferências granulares de notificações do aplicativo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreferenciasNotificacoes {
    /// Habilita notificações nativas do sistema operacional (além dos alertas na interface).
    pub habilitar_notificacoes_sistema: bool,
    /// Notificar ao concluir um vídeo individual.
    pub processamento_concluido: bool,
    /// Notificar quando um vídeo falhar.
    pub processamento_com_falhas: bool,
    /// Notificar quando toda a fila terminar.
    pub fila_finalizada: bool,
    /// Notificar em caso de espaço em disco insuficiente.
    pub falta_espaco_disco: bool,
    /// Notificar quando o processamento for cancelado.
    pub cancelamento: bool,
    /// Notificar quando FFmpeg ou FFprobe estiverem indisponíveis.
    pub dependencias_indisponiveis: bool,
}

impl Default for PreferenciasNotificacoes {
    fn default() -> Self {
        Self {
            habilitar_notificacoes_sistema: true,
            processamento_concluido: false,
            processamento_com_falhas: true,
            fila_finalizada: true,
            falta_espaco_disco: true,
            cancelamento: true,
            dependencias_indisponiveis: true,
        }
    }
}

/// Dimensões e posição persistidas da janela desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensoesJanela {
    /// Largura da janela em pontos lógicos.
    pub largura: f32,
    /// Altura da janela em pontos lógicos.
    pub altura: f32,
    /// Posição horizontal salva, quando disponível.
    pub pos_x: Option<f32>,
    /// Posição vertical salva, quando disponível.
    pub pos_y: Option<f32>,
    /// Indica se a janela estava maximizada ao encerrar.
    pub maximizada: bool,
}

impl Default for DimensoesJanela {
    fn default() -> Self {
        Self {
            largura: 1180.0,
            altura: 760.0,
            pos_x: None,
            pos_y: None,
            maximizada: false,
        }
    }
}

/// Conjunto completo de configurações persistentes do aplicativo desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfiguracoesPersistentes {
    /// Tema visual escolhido (`Sistema`, `Escuro` ou `Claro`).
    pub tema: TemaPreferido,
    /// Idioma da interface (padrão: `"pt-BR"`).
    pub idioma: String,
    /// Opção predefinida ou personalizada do limite máximo por segmento.
    pub limite_predefinido: LimitePredefinido,
    /// Valor personalizado em bytes (usado quando `limite_predefinido == Personalizado`).
    pub limite_personalizado_bytes: u64,
    /// Diretório raiz padrão onde as pastas dos vídeos divididos serão criadas.
    pub diretorio_saida: PathBuf,
    /// Quantidade de vídeos processados simultaneamente.
    pub concorrencia: usize,
    /// Autorizar sobrescrita de segmentos já existentes na pasta de saída.
    pub permitir_sobrescrita: bool,
    /// Abrir automaticamente a pasta de saída ao concluir o processamento.
    pub abrir_pasta_ao_concluir: bool,
    /// Percorrer subdiretórios recursivamente ao selecionar/soltar pastas.
    pub busca_recursiva: bool,
    /// Caminho personalizado opcional para o executável `ffmpeg`.
    pub caminho_ffmpeg: Option<PathBuf>,
    /// Caminho personalizado opcional para o executável `ffprobe`.
    pub caminho_ffprobe: Option<PathBuf>,
    /// Preferências de notificações.
    pub notificacoes: PreferenciasNotificacoes,
    /// Dimensões persistidas da janela principal.
    pub janela: DimensoesJanela,
}

impl Default for ConfiguracoesPersistentes {
    fn default() -> Self {
        Self {
            tema: TemaPreferido::Sistema,
            idioma: "pt-BR".to_string(),
            limite_predefinido: LimitePredefinido::Mb10,
            limite_personalizado_bytes: TAMANHO_MAXIMO_PADRAO_BYTES,
            diretorio_saida: diretorio_saida_padrao(),
            concorrencia: calcular_concorrencia_padrao(),
            permitir_sobrescrita: false,
            abrir_pasta_ao_concluir: false,
            busca_recursiva: false,
            caminho_ffmpeg: None,
            caminho_ffprobe: None,
            notificacoes: PreferenciasNotificacoes::default(),
            janela: DimensoesJanela::default(),
        }
    }
}

impl ConfiguracoesPersistentes {
    /// Caminho padrão do arquivo de configurações no diretório apropriado do SO.
    pub fn caminho_padrao() -> PathBuf {
        diretorio_configuracoes().join(NOME_ARQUIVO_CONFIGURACOES)
    }

    /// Retorna o limite máximo efetivo em bytes (respeitando unidades decimais).
    pub fn limite_maximo_efetivo_bytes(&self) -> u64 {
        self.limite_predefinido
            .bytes_fixos()
            .unwrap_or(self.limite_personalizado_bytes)
            .max(100_000)
    }

    /// Retorna o tamanho-alvo efetivo (com margem de segurança calculada automaticamente).
    pub fn tamanho_alvo_efetivo_bytes(&self) -> u64 {
        calcular_tamanho_alvo_adaptativo(self.limite_maximo_efetivo_bytes())
    }

    /// Retorna a margem de segurança em bytes para exibição informativa na interface.
    pub fn margem_seguranca_bytes(&self) -> u64 {
        self.limite_maximo_efetivo_bytes()
            .saturating_sub(self.tamanho_alvo_efetivo_bytes())
    }

    /// Carrega as configurações do caminho padrão do sistema (ou retorna padrão se não existir).
    pub fn carregar_padrao() -> Self {
        let caminho = Self::caminho_padrao();
        Self::carregar_de(&caminho).unwrap_or_default()
    }

    /// Salva as configurações no caminho padrão do sistema operacional.
    pub fn salvar_padrao(&self) -> Resultado<()> {
        self.salvar_em(&Self::caminho_padrao())
    }

    /// Carrega configurações de um arquivo JSON específico.
    pub fn carregar_de(caminho: &Path) -> Resultado<Self> {
        if !caminho.exists() {
            return Ok(Self::default());
        }
        let conteudo = fs::read_to_string(caminho).map_err(|erro| {
            ErroDivisor::io(
                format!("leitura das configurações em '{}'", caminho.display()),
                erro,
            )
        })?;
        let mut cfg: Self = serde_json::from_str(&conteudo).map_err(|erro| {
            ErroDivisor::FalhaInterpretacaoJson {
                detalhe: format!("Erro ao interpretar '{}': {erro}", caminho.display()),
            }
        })?;
        cfg.normalizar();
        Ok(cfg)
    }

    /// Salva configurações em disco de forma atômica (grava em `.tmp` e renomeia).
    pub fn salvar_em(&self, caminho: &Path) -> Resultado<()> {
        if let Some(pai) = caminho.parent() {
            garantir_diretorio(pai)?;
        }

        let json = serde_json::to_string_pretty(self).map_err(|erro| {
            ErroDivisor::FalhaInterpretacaoJson {
                detalhe: format!("Erro ao serializar configurações: {erro}"),
            }
        })?;

        let tmp = caminho.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|erro| {
            ErroDivisor::io(
                format!("escrita temporária de configurações em '{}'", tmp.display()),
                erro,
            )
        })?;

        fs::rename(&tmp, caminho).map_err(|erro| {
            let _ = fs::remove_file(&tmp);
            ErroDivisor::io(
                format!("publicação das configurações em '{}'", caminho.display()),
                erro,
            )
        })?;

        Ok(())
    }

    /// Garante limites sãos para campos numéricos após desserialização.
    fn normalizar(&mut self) {
        self.concorrencia = self.concorrencia.clamp(1, 16);
        self.limite_personalizado_bytes = self.limite_personalizado_bytes.max(100_000);
        self.janela.largura = self.janela.largura.clamp(780.0, 7680.0);
        self.janela.altura = self.janela.altura.clamp(520.0, 4320.0);
        if self.idioma.trim().is_empty() {
            self.idioma = "pt-BR".to_string();
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn calcula_margem_adaptativa_em_unidades_decimais() {
        assert_eq!(calcular_tamanho_alvo_adaptativo(10_000_000), 9_800_000);
        assert_eq!(calcular_tamanho_alvo_adaptativo(5_000_000), 4_900_000);
        assert_eq!(calcular_tamanho_alvo_adaptativo(50_000_000), 49_250_000);
        assert_eq!(calcular_tamanho_alvo_adaptativo(1_000_000_000), 996_000_000);

        for opcao in LimitePredefinido::TODAS {
            if let Some(lim) = opcao.bytes_fixos() {
                let alvo = calcular_tamanho_alvo_adaptativo(lim);
                assert!(alvo < lim, "alvo deve manter margem para {opcao:?}");
                assert!(alvo > 0);
            }
        }
    }

    #[test]
    fn persiste_e_carrega_configuracoes_atomicamente() {
        let dir = tempfile::tempdir().ok();
        assert!(dir.is_some());
        if let Some(d) = dir {
            let arquivo = d.path().join("cfg.json");
            let mut cfg = ConfiguracoesPersistentes {
                tema: TemaPreferido::Escuro,
                limite_predefinido: LimitePredefinido::Mb50,
                concorrencia: 3,
                abrir_pasta_ao_concluir: true,
                busca_recursiva: true,
                ..Default::default()
            };
            cfg.janela.largura = 1366.0;
            cfg.janela.altura = 820.0;

            assert!(cfg.salvar_em(&arquivo).is_ok());
            let carregado = ConfiguracoesPersistentes::carregar_de(&arquivo);
            assert!(carregado.is_ok());
            if let Ok(lido) = carregado {
                assert_eq!(lido, cfg);
                assert_eq!(lido.limite_maximo_efetivo_bytes(), 50_000_000);
            }
        }
    }
}
