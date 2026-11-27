//! Integração segura e multiplataforma com o sistema operacional.
//!
//! Provê descoberta de diretórios padrão do sistema (Linux XDG, Windows AppData, macOS Library),
//! resolução ordenada de ferramentas portáteis (`ffmpeg`/`ffprobe`), verificação de espaço livre
//! em disco e abertura nativa de arquivos e diretórios sem invocação de shell.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::erros::{ErroDivisor, Resultado};

/// Nome padronizado da pasta de dados/configuração do aplicativo nos diretórios do usuário.
pub const NOME_PASTA_APLICATIVO: &str = "divisor_video";

/// Subdiretórios verificados ao lado do executável na distribuição portátil.
const SUBDIRETORIOS_FERRAMENTAS: &[&str] = &["ferramentas", "tools", "bin"];

/// Origem onde um executável multimídia (`ffmpeg` ou `ffprobe`) foi encontrado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrigemFerramenta {
    /// Caminho explicitamente configurado pelo usuário.
    Personalizado,
    /// Mesmo diretório do executável do aplicativo.
    DiretorioAplicativo,
    /// Subdiretório de ferramentas (`ferramentas/`, `tools/`, `bin/`) ao lado do executável.
    SubdiretorioFerramentas,
    /// Variável de ambiente `PATH` do sistema operacional.
    PathSistema,
}

impl OrigemFerramenta {
    /// Descrição legível em português brasileiro para exibição em diagnósticos.
    pub fn descricao(self) -> &'static str {
        match self {
            Self::Personalizado => "Caminho personalizado",
            Self::DiretorioAplicativo => "Diretório do aplicativo",
            Self::SubdiretorioFerramentas => "Pasta de ferramentas do aplicativo",
            Self::PathSistema => "PATH do sistema operacional",
        }
    }
}

/// Retorna o diretório apropriado para salvar configurações persistentes no sistema operacional.
pub fn diretorio_configuracoes() -> PathBuf {
    dirs::config_dir()
        .map(|base| base.join(NOME_PASTA_APLICATIVO))
        .unwrap_or_else(|| PathBuf::from(".").join(".config_divisor_video"))
}

/// Retorna o diretório apropriado para dados locais do aplicativo (ex: banco SQLite de histórico).
pub fn diretorio_dados() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .map(|base| base.join(NOME_PASTA_APLICATIVO))
        .unwrap_or_else(diretorio_configuracoes)
}

/// Retorna o diretório de cache local para miniaturas geradas pelo FFmpeg.
pub fn diretorio_cache_miniaturas() -> PathBuf {
    dirs::cache_dir()
        .map(|base| base.join(NOME_PASTA_APLICATIVO).join("miniaturas"))
        .unwrap_or_else(|| diretorio_dados().join("miniaturas"))
}

/// Retorna o diretório de saída padrão amigável ao usuário (ex: `~/Vídeos/DivisorVideo` ou `./saida`).
pub fn diretorio_saida_padrao() -> PathBuf {
    dirs::video_dir()
        .or_else(dirs::home_dir)
        .map(|base| base.join("DivisorVideo"))
        .unwrap_or_else(|| PathBuf::from("saida"))
}

/// Nome do executável ajustado para a plataforma atual (`.exe` no Windows).
pub fn nome_executavel_plataforma(nome_base: &str) -> String {
    if cfg!(target_os = "windows") && !nome_base.to_ascii_lowercase().ends_with(".exe") {
        format!("{nome_base}.exe")
    } else {
        nome_base.to_string()
    }
}

/// Gera a lista ordenada de candidatos para localizar `ffmpeg` ou `ffprobe` seguindo a prioridade:
/// 1. Caminho personalizado nas configurações.
/// 2. Diretório do executável do aplicativo.
/// 3. Subdiretórios de ferramentas do aplicativo (`ferramentas/`, `tools/`, `bin/`).
/// 4. `PATH` do sistema operacional.
pub fn listar_candidatos_ferramenta(
    nome_base: &str,
    caminho_personalizado: Option<&Path>,
    dir_executavel_app: Option<&Path>,
) -> Vec<(PathBuf, OrigemFerramenta)> {
    let mut candidatos = Vec::new();
    let nome_bin = nome_executavel_plataforma(nome_base);

    // 1. Caminho personalizado
    if let Some(personalizado) = caminho_personalizado
        && !personalizado.as_os_str().is_empty()
    {
        candidatos.push((personalizado.to_path_buf(), OrigemFerramenta::Personalizado));
    }

    // 2 e 3. Diretório do aplicativo e subdiretórios de ferramentas
    let dir_app_resolvido = dir_executavel_app.map(Path::to_path_buf).or_else(|| {
        env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
    });

    if let Some(dir_app) = dir_app_resolvido {
        let direto = dir_app.join(&nome_bin);
        if direto.is_file() {
            candidatos.push((direto, OrigemFerramenta::DiretorioAplicativo));
        }

        for sub in SUBDIRETORIOS_FERRAMENTAS {
            let em_sub = dir_app.join(sub).join(&nome_bin);
            if em_sub.is_file() {
                candidatos.push((em_sub, OrigemFerramenta::SubdiretorioFerramentas));
            }
        }
    }

    // 4. PATH do sistema operacional
    candidatos.push((PathBuf::from(nome_base), OrigemFerramenta::PathSistema));

    candidatos
}

/// Consulta o espaço livre disponível (em bytes) no volume que contém `caminho`
/// (subindo na hierarquia caso o diretório ainda não exista).
pub fn espaco_livre_bytes(caminho: &Path) -> Resultado<u64> {
    let mut atual = caminho.to_path_buf();
    while !atual.exists() {
        if let Some(pai) = atual.parent() {
            if pai.as_os_str().is_empty() {
                atual = PathBuf::from(".");
                break;
            }
            atual = pai.to_path_buf();
        } else {
            atual = PathBuf::from(".");
            break;
        }
    }

    fs2::available_space(&atual).map_err(|erro| {
        ErroDivisor::io(
            format!("consulta de espaço em disco em '{}'", atual.display()),
            erro,
        )
    })
}

/// Verifica se há espaço livre suficiente no diretório de saída para processar um vídeo
/// de `tamanho_video_bytes`, reservando folga de segurança de 5% + 16 MB para overhead e temporários.
pub fn verificar_espaco_para_video(
    diretorio_saida: &Path,
    tamanho_video_bytes: u64,
) -> Resultado<u64> {
    let disponivel = espaco_livre_bytes(diretorio_saida)?;
    let margem = (tamanho_video_bytes / 20).saturating_add(16_000_000);
    let necessario = tamanho_video_bytes.saturating_add(margem);

    if disponivel < necessario {
        return Err(ErroDivisor::EspacoEmDiscoInsuficiente {
            caminho: diretorio_saida.to_path_buf(),
            disponivel_bytes: disponivel,
            necessario_bytes: necessario,
        });
    }

    Ok(disponivel)
}

/// Abre um arquivo (no reprodutor padrão) ou um diretório (no gerenciador de arquivos nativo)
/// de forma desacoplada e segura, sem uso de shell intermediário.
pub fn abrir_no_sistema(caminho: &Path) -> Resultado<()> {
    if !caminho.exists() {
        return Err(ErroDivisor::ArquivoNaoEncontrado {
            caminho: caminho.to_path_buf(),
        });
    }

    open::that_detached(caminho).map_err(|erro| ErroDivisor::ErroSistemaOperacional {
        detalhe: format!("Não foi possível abrir '{}': {erro}", caminho.display()),
    })
}

/// Garante a existência de um diretório criando pais se necessário.
pub fn garantir_diretorio(caminho: &Path) -> Resultado<()> {
    fs::create_dir_all(caminho).map_err(|erro| {
        ErroDivisor::io(
            format!("criação do diretório '{}'", caminho.display()),
            erro,
        )
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn ordem_deteccao_ferramentas_respeita_prioridade_1_a_4() {
        let tmp = tempfile::tempdir().ok();
        assert!(tmp.is_some());
        if let Some(dir) = tmp {
            let nome_bin = nome_executavel_plataforma("ffmpeg");
            let bin_app = dir.path().join(&nome_bin);
            let sub_dir = dir.path().join("ferramentas");
            let _ = fs::create_dir_all(&sub_dir);
            let bin_sub = sub_dir.join(&nome_bin);
            let _ = fs::write(&bin_app, b"fake");
            let _ = fs::write(&bin_sub, b"fake");

            let personalizado = dir.path().join("custom_ffmpeg");
            let lista =
                listar_candidatos_ferramenta("ffmpeg", Some(&personalizado), Some(dir.path()));

            assert_eq!(lista.len(), 4);
            assert_eq!(lista[0].1, OrigemFerramenta::Personalizado);
            assert_eq!(lista[1].1, OrigemFerramenta::DiretorioAplicativo);
            assert_eq!(lista[2].1, OrigemFerramenta::SubdiretorioFerramentas);
            assert_eq!(lista[3].1, OrigemFerramenta::PathSistema);
        }
    }

    #[test]
    fn consulta_espaco_livre_em_diretorio_existente_e_inexistente() {
        let tmp = tempfile::tempdir().ok();
        assert!(tmp.is_some());
        if let Some(dir) = tmp {
            let inexistente = dir.path().join("sub1").join("sub2");
            let livre = espaco_livre_bytes(&inexistente);
            assert!(livre.is_ok());
            assert!(livre.unwrap_or(0) > 0);
        }
    }
}
