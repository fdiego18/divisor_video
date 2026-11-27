//! Persistência de histórico de processamentos em banco de dados SQLite local (`rusqlite`).
//!
//! Suporta consulta filtrada por nome, intervalo de datas e status, exclusão individual
//! ou limpeza completa (sem jamais remover arquivos de vídeo ou segmentos do disco)
//! e exportação estruturada em CSV e JSON.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::erros::{ErroDivisor, Resultado};
use crate::eventos::InfoSegmentoGerado;
use crate::integracao_so::{diretorio_dados, garantir_diretorio};

/// Nome do arquivo do banco de dados SQLite local.
const NOME_BANCO_HISTORICO: &str = "historico.sqlite3";

/// Registro completo armazenado no histórico de processamentos.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistroHistorico {
    /// Identificador único do processamento no banco SQLite.
    pub id: i64,
    /// Nome do arquivo de vídeo original.
    pub nome_video: String,
    /// Caminho completo do vídeo original.
    pub caminho_video: PathBuf,
    /// Data e hora do processamento no formato `YYYY-MM-DD HH:MM:SS`.
    pub data_hora: String,
    /// Tamanho do arquivo original em bytes.
    pub tamanho_original_bytes: u64,
    /// Limite máximo configurado em bytes no momento do processamento.
    pub limite_configurado_bytes: u64,
    /// Quantidade de segmentos gerados.
    pub quantidade_segmentos: usize,
    /// Lista detalhada (nome, caminho, tamanho real e duração) de cada segmento gerado.
    pub segmentos: Vec<InfoSegmentoGerado>,
    /// Diretório onde os segmentos foram salvos, quando aplicável.
    pub diretorio_saida: Option<PathBuf>,
    /// Tempo total de processamento em milissegundos.
    pub tempo_processamento_ms: u64,
    /// Velocidade média em MB/s.
    pub velocidade_media_mb_s: f64,
    /// Resultado final (`Concluído`, `Ignorado`, `Cancelado`, `Erro`).
    pub resultado: String,
    /// Mensagem de erro detalhada, quando aplicável.
    pub mensagem_erro: Option<String>,
}

/// Dados de entrada para inserção de um novo registro no histórico.
#[derive(Debug, Clone)]
pub struct NovoRegistroHistorico {
    pub nome_video: String,
    pub caminho_video: PathBuf,
    pub data_hora: Option<String>,
    pub tamanho_original_bytes: u64,
    pub limite_configurado_bytes: u64,
    pub segmentos: Vec<InfoSegmentoGerado>,
    pub diretorio_saida: Option<PathBuf>,
    pub tempo_processamento_ms: u64,
    pub velocidade_media_mb_s: f64,
    pub resultado: String,
    pub mensagem_erro: Option<String>,
}

/// Critérios de pesquisa e filtragem sobre o histórico.
#[derive(Debug, Clone, Default)]
pub struct FiltroHistorico {
    /// Texto parcial para pesquisa por nome ou caminho do vídeo (case-insensitive).
    pub termo_nome: String,
    /// Data inicial opcional no formato `YYYY-MM-DD` (inclusiva).
    pub data_inicio: String,
    /// Data final opcional no formato `YYYY-MM-DD` (inclusiva).
    pub data_fim: String,
    /// Filtro exato por status (`None` significa todos).
    pub status: Option<String>,
}

/// Repositório SQLite para leitura, escrita e exportação do histórico.
pub struct RepositorioHistorico {
    conexao: Connection,
    caminho_banco: PathBuf,
}

impl RepositorioHistorico {
    /// Caminho padrão do banco SQLite no diretório local de dados do usuário.
    pub fn caminho_padrao() -> PathBuf {
        diretorio_dados().join(NOME_BANCO_HISTORICO)
    }

    /// Abre (ou cria) o banco de histórico no caminho padrão do sistema operacional.
    pub fn abrir_padrao() -> Resultado<Self> {
        Self::abrir(&Self::caminho_padrao())
    }

    /// Abre (ou cria) um banco de histórico em um caminho específico.
    pub fn abrir(caminho_banco: &Path) -> Resultado<Self> {
        if let Some(pai) = caminho_banco.parent() {
            garantir_diretorio(pai)?;
        }

        let conexao =
            Connection::open(caminho_banco).map_err(|erro| ErroDivisor::ErroBancoDados {
                detalhe: format!(
                    "Falha ao abrir banco SQLite em '{}': {erro}",
                    caminho_banco.display()
                ),
            })?;

        conexao
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS processamentos (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     nome_video TEXT NOT NULL,
                     caminho_video TEXT NOT NULL,
                     data_hora TEXT NOT NULL,
                     tamanho_original_bytes INTEGER NOT NULL,
                     limite_configurado_bytes INTEGER NOT NULL,
                     quantidade_segmentos INTEGER NOT NULL,
                     segmentos_json TEXT NOT NULL,
                     diretorio_saida TEXT,
                     tempo_processamento_ms INTEGER NOT NULL,
                     velocidade_media_mb_s REAL NOT NULL,
                     resultado TEXT NOT NULL,
                     mensagem_erro TEXT
                 );
                 CREATE INDEX IF NOT EXISTS idx_proc_data ON processamentos(data_hora DESC);
                 CREATE INDEX IF NOT EXISTS idx_proc_resultado ON processamentos(resultado);",
            )
            .map_err(|erro| ErroDivisor::ErroBancoDados {
                detalhe: format!("Falha ao inicializar esquema do histórico: {erro}"),
            })?;

        Ok(Self {
            conexao,
            caminho_banco: caminho_banco.to_path_buf(),
        })
    }

    /// Caminho em disco do banco SQLite em uso.
    pub fn caminho_banco(&self) -> &Path {
        &self.caminho_banco
    }

    /// Insere um novo registro de processamento e retorna o identificador gerado.
    pub fn inserir(&self, novo: &NovoRegistroHistorico) -> Resultado<i64> {
        let data_hora = novo
            .data_hora
            .clone()
            .unwrap_or_else(|| Local::now().format("%Y-%m-%d %H:%M:%S").to_string());

        let segmentos_json = serde_json::to_string(&novo.segmentos).map_err(|erro| {
            ErroDivisor::FalhaInterpretacaoJson {
                detalhe: format!("Falha ao serializar segmentos para o histórico: {erro}"),
            }
        })?;

        let tam_orig = i64::try_from(novo.tamanho_original_bytes).unwrap_or(i64::MAX);
        let lim_cfg = i64::try_from(novo.limite_configurado_bytes).unwrap_or(i64::MAX);
        let qtd_segs = i64::try_from(novo.segmentos.len()).unwrap_or(i64::MAX);
        let tempo_ms = i64::try_from(novo.tempo_processamento_ms).unwrap_or(i64::MAX);
        let dir_saida_str = novo
            .diretorio_saida
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());

        self.conexao
            .execute(
                "INSERT INTO processamentos (
                    nome_video, caminho_video, data_hora, tamanho_original_bytes,
                    limite_configurado_bytes, quantidade_segmentos, segmentos_json,
                    diretorio_saida, tempo_processamento_ms, velocidade_media_mb_s,
                    resultado, mensagem_erro
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    novo.nome_video,
                    novo.caminho_video.to_string_lossy().into_owned(),
                    data_hora,
                    tam_orig,
                    lim_cfg,
                    qtd_segs,
                    segmentos_json,
                    dir_saida_str,
                    tempo_ms,
                    novo.velocidade_media_mb_s,
                    novo.resultado,
                    novo.mensagem_erro,
                ],
            )
            .map_err(|erro| ErroDivisor::ErroBancoDados {
                detalhe: format!("Falha ao inserir registro no histórico: {erro}"),
            })?;

        Ok(self.conexao.last_insert_rowid())
    }

    /// Consulta registros do histórico aplicando filtros opcionais de nome, data e status,
    /// ordenados do mais recente para o mais antigo.
    pub fn consultar(&self, filtro: &FiltroHistorico) -> Resultado<Vec<RegistroHistorico>> {
        let mut sql = String::from(
            "SELECT id, nome_video, caminho_video, data_hora, tamanho_original_bytes,
                    limite_configurado_bytes, quantidade_segmentos, segmentos_json,
                    diretorio_saida, tempo_processamento_ms, velocidade_media_mb_s,
                    resultado, mensagem_erro
             FROM processamentos
             WHERE 1 = 1",
        );

        let mut parametros: Vec<String> = Vec::new();

        let termo = filtro.termo_nome.trim();
        if !termo.is_empty() {
            let idx = parametros.len() + 1;
            sql.push_str(&format!(
                " AND (LOWER(nome_video) LIKE LOWER(?{idx}) OR LOWER(caminho_video) LIKE LOWER(?{idx}))"
            ));
            parametros.push(format!("%{termo}%"));
        }

        let data_ini = filtro.data_inicio.trim();
        if !data_ini.is_empty() {
            let idx = parametros.len() + 1;
            sql.push_str(&format!(" AND substr(data_hora, 1, 10) >= ?{idx}"));
            parametros.push(data_ini.to_string());
        }

        let data_fim = filtro.data_fim.trim();
        if !data_fim.is_empty() {
            let idx = parametros.len() + 1;
            sql.push_str(&format!(" AND substr(data_hora, 1, 10) <= ?{idx}"));
            parametros.push(data_fim.to_string());
        }

        if let Some(status) = &filtro.status {
            let st = status.trim();
            if !st.is_empty() {
                let idx = parametros.len() + 1;
                sql.push_str(&format!(" AND resultado = ?{idx}"));
                parametros.push(st.to_string());
            }
        }

        sql.push_str(" ORDER BY id DESC LIMIT 1000");

        let mut stmt = self
            .conexao
            .prepare(&sql)
            .map_err(|erro| ErroDivisor::ErroBancoDados {
                detalhe: format!("Falha ao preparar consulta do histórico: {erro}"),
            })?;

        let linhas = stmt
            .query_map(rusqlite::params_from_iter(parametros.iter()), |row| {
                let id: i64 = row.get(0)?;
                let nome_video: String = row.get(1)?;
                let caminho_str: String = row.get(2)?;
                let data_hora: String = row.get(3)?;
                let tam_orig: i64 = row.get(4)?;
                let lim_cfg: i64 = row.get(5)?;
                let qtd_segs: i64 = row.get(6)?;
                let segmentos_json: String = row.get(7)?;
                let dir_saida: Option<String> = row.get(8)?;
                let tempo_ms: i64 = row.get(9)?;
                let vel_mb_s: f64 = row.get(10)?;
                let resultado: String = row.get(11)?;
                let mensagem_erro: Option<String> = row.get(12)?;

                let segmentos: Vec<InfoSegmentoGerado> =
                    serde_json::from_str(&segmentos_json).unwrap_or_default();

                Ok(RegistroHistorico {
                    id,
                    nome_video,
                    caminho_video: PathBuf::from(caminho_str),
                    data_hora,
                    tamanho_original_bytes: u64::try_from(tam_orig).unwrap_or(0),
                    limite_configurado_bytes: u64::try_from(lim_cfg).unwrap_or(0),
                    quantidade_segmentos: usize::try_from(qtd_segs).unwrap_or(segmentos.len()),
                    segmentos,
                    diretorio_saida: dir_saida.map(PathBuf::from),
                    tempo_processamento_ms: u64::try_from(tempo_ms).unwrap_or(0),
                    velocidade_media_mb_s: vel_mb_s,
                    resultado,
                    mensagem_erro,
                })
            })
            .map_err(|erro| ErroDivisor::ErroBancoDados {
                detalhe: format!("Falha ao executar consulta do histórico: {erro}"),
            })?;

        let mut registros = Vec::new();
        for linha in linhas {
            let item = linha.map_err(|erro| ErroDivisor::ErroBancoDados {
                detalhe: format!("Falha ao ler linha do histórico: {erro}"),
            })?;
            registros.push(item);
        }

        Ok(registros)
    }

    /// Exclui um registro específico do histórico.
    ///
    /// Esta operação afeta somente o banco SQLite e nunca remove vídeos originais nem segmentos.
    pub fn excluir_registro(&self, id: i64) -> Resultado<()> {
        self.conexao
            .execute("DELETE FROM processamentos WHERE id = ?1", params![id])
            .map_err(|erro| ErroDivisor::ErroBancoDados {
                detalhe: format!("Falha ao excluir registro #{id} do histórico: {erro}"),
            })?;
        Ok(())
    }

    /// Remove todos os registros do histórico.
    ///
    /// Esta operação afeta somente o banco SQLite e nunca remove vídeos originais nem segmentos.
    pub fn limpar_tudo(&self) -> Resultado<usize> {
        let removidos = self
            .conexao
            .execute("DELETE FROM processamentos", [])
            .map_err(|erro| ErroDivisor::ErroBancoDados {
                detalhe: format!("Falha ao limpar histórico: {erro}"),
            })?;
        Ok(removidos)
    }

    /// Exporta os registros filtrados em formato JSON legível.
    pub fn exportar_json(&self, filtro: &FiltroHistorico, destino: &Path) -> Resultado<usize> {
        let registros = self.consultar(filtro)?;
        let conteudo = serde_json::to_string_pretty(&registros).map_err(|erro| {
            ErroDivisor::FalhaInterpretacaoJson {
                detalhe: format!("Falha ao serializar histórico em JSON: {erro}"),
            }
        })?;

        fs::write(destino, conteudo).map_err(|erro| {
            ErroDivisor::io(
                format!("exportação do histórico JSON para '{}'", destino.display()),
                erro,
            )
        })?;
        Ok(registros.len())
    }

    /// Exporta os registros filtrados em formato CSV compatível com planilhas (RFC 4180).
    pub fn exportar_csv(&self, filtro: &FiltroHistorico, destino: &Path) -> Resultado<usize> {
        let registros = self.consultar(filtro)?;
        let mut csv = String::from(
            "id,data_hora,nome_video,caminho_video,tamanho_original_bytes,limite_configurado_bytes,quantidade_segmentos,tamanhos_segmentos_bytes,diretorio_saida,tempo_processamento_ms,velocidade_media_mb_s,resultado,mensagem_erro\n",
        );

        for reg in &registros {
            let tamanhos_segs = reg
                .segmentos
                .iter()
                .map(|s| s.tamanho_bytes.to_string())
                .collect::<Vec<_>>()
                .join(";");
            let dir_saida = reg
                .diretorio_saida
                .as_ref()
                .map_or_else(String::new, |p| p.to_string_lossy().into_owned());
            let erro = reg.mensagem_erro.as_deref().unwrap_or("");

            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{:.2},{},{}\n",
                reg.id,
                escapar_csv(&reg.data_hora),
                escapar_csv(&reg.nome_video),
                escapar_csv(&reg.caminho_video.to_string_lossy()),
                reg.tamanho_original_bytes,
                reg.limite_configurado_bytes,
                reg.quantidade_segmentos,
                escapar_csv(&tamanhos_segs),
                escapar_csv(&dir_saida),
                reg.tempo_processamento_ms,
                reg.velocidade_media_mb_s,
                escapar_csv(&reg.resultado),
                escapar_csv(erro),
            ));
        }

        fs::write(destino, csv).map_err(|erro| {
            ErroDivisor::io(
                format!("exportação do histórico CSV para '{}'", destino.display()),
                erro,
            )
        })?;
        Ok(registros.len())
    }
}

/// Escapa um campo de texto segundo a RFC 4180 para exportação CSV segura.
fn escapar_csv(valor: &str) -> String {
    if valor.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", valor.replace('"', "\"\""))
    } else {
        valor.to_string()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn criar_registro_exemplo(nome: &str, data: &str, resultado: &str) -> NovoRegistroHistorico {
        NovoRegistroHistorico {
            nome_video: nome.to_string(),
            caminho_video: PathBuf::from(format!("/videos/{nome}")),
            data_hora: Some(format!("{data} 14:30:00")),
            tamanho_original_bytes: 25_000_000,
            limite_configurado_bytes: 10_000_000,
            segmentos: vec![
                InfoSegmentoGerado {
                    indice: 1,
                    nome_arquivo: format!("{nome}_parte_001.mp4"),
                    caminho: PathBuf::from(format!("/saida/{nome}_parte_001.mp4")),
                    tamanho_bytes: 9_600_000,
                    duracao_segundos: 10.0,
                },
                InfoSegmentoGerado {
                    indice: 2,
                    nome_arquivo: format!("{nome}_parte_002.mp4"),
                    caminho: PathBuf::from(format!("/saida/{nome}_parte_002.mp4")),
                    tamanho_bytes: 9_500_000,
                    duracao_segundos: 10.0,
                },
            ],
            diretorio_saida: Some(PathBuf::from("/saida")),
            tempo_processamento_ms: 850,
            velocidade_media_mb_s: 29.41,
            resultado: resultado.to_string(),
            mensagem_erro: (resultado == "Erro").then(|| "GOP indivisível".to_string()),
        }
    }

    #[test]
    fn crud_filtros_e_exportacao_sqlite() {
        let dir = tempfile::tempdir().ok();
        assert!(dir.is_some());
        if let Some(d) = dir {
            let db_path = d.path().join("teste.sqlite3");
            let repo = RepositorioHistorico::abrir(&db_path);
            assert!(repo.is_ok());

            if let Ok(repo) = repo {
                let id1 = repo
                    .inserir(&criar_registro_exemplo(
                        "ferias.mp4",
                        "2026-11-20",
                        "Concluído",
                    ))
                    .unwrap_or(0);
                let _id2 = repo
                    .inserir(&criar_registro_exemplo("reuniao.mkv", "2026-11-25", "Erro"))
                    .unwrap_or(0);
                assert!(id1 > 0);

                // Filtro por nome
                let filtro_nome = FiltroHistorico {
                    termo_nome: "FERIAS".to_string(),
                    ..Default::default()
                };
                let res_nome = repo.consultar(&filtro_nome).unwrap_or_default();
                assert_eq!(res_nome.len(), 1);
                assert_eq!(res_nome[0].segmentos.len(), 2);

                // Filtro por status
                let filtro_erro = FiltroHistorico {
                    status: Some("Erro".to_string()),
                    ..Default::default()
                };
                let res_erro = repo.consultar(&filtro_erro).unwrap_or_default();
                assert_eq!(res_erro.len(), 1);
                assert_eq!(res_erro[0].nome_video, "reuniao.mkv");

                // Filtro por data
                let filtro_data = FiltroHistorico {
                    data_inicio: "2026-11-24".to_string(),
                    data_fim: "2026-11-26".to_string(),
                    ..Default::default()
                };
                let res_data = repo.consultar(&filtro_data).unwrap_or_default();
                assert_eq!(res_data.len(), 1);

                // Exportação CSV e JSON
                let csv_path = d.path().join("hist.csv");
                let json_path = d.path().join("hist.json");
                assert_eq!(
                    repo.exportar_csv(&FiltroHistorico::default(), &csv_path)
                        .unwrap_or(0),
                    2
                );
                assert_eq!(
                    repo.exportar_json(&FiltroHistorico::default(), &json_path)
                        .unwrap_or(0),
                    2
                );
                assert!(
                    fs::read_to_string(&csv_path)
                        .unwrap_or_default()
                        .contains("ferias.mp4")
                );
                assert!(
                    fs::read_to_string(&json_path)
                        .unwrap_or_default()
                        .contains("reuniao.mkv")
                );

                // Exclusão individual e limpeza total
                assert!(repo.excluir_registro(id1).is_ok());
                assert_eq!(
                    repo.consultar(&FiltroHistorico::default())
                        .unwrap_or_default()
                        .len(),
                    1
                );
                assert_eq!(repo.limpar_tudo().unwrap_or(0), 1);
                assert!(
                    repo.consultar(&FiltroHistorico::default())
                        .unwrap_or_default()
                        .is_empty()
                );
            }
        }
    }
}
