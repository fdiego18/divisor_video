//! Gerenciamento de barras de progresso, métricas de velocidade e exibição de relatórios.

use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Controlador compartilhado de barras de progresso para execução individual ou em lote.
#[derive(Clone)]
pub struct GerenciadorProgresso {
    multi: MultiProgress,
    barra_geral: Option<ProgressBar>,
    silencioso: bool,
}

impl GerenciadorProgresso {
    /// Cria um gerenciador de progresso para a quantidade total de vídeos descobertos.
    pub fn novo(total_videos: usize, silencioso: bool) -> Self {
        let multi = MultiProgress::new();
        let barra_geral = if !silencioso && total_videos > 1 {
            let pb = multi.add(ProgressBar::new(total_videos as u64));
            let estilo = ProgressStyle::with_template(
                "{spinner:.cyan} Lote [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} vídeos ({msg})",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("█▓░");
            pb.set_style(estilo);
            pb.set_message("em andamento");
            pb.enable_steady_tick(Duration::from_millis(120));
            Some(pb)
        } else {
            None
        };

        Self {
            multi,
            barra_geral,
            silencioso,
        }
    }

    /// Cria uma barra de progresso dedicada para um arquivo de vídeo específico.
    pub fn criar_barra_video(&self, nome_arquivo: &str, tamanho_bytes: u64) -> BarraVideo {
        if self.silencioso {
            return BarraVideo {
                barra: ProgressBar::hidden(),
                tamanho_total: tamanho_bytes,
            };
        }

        let pb = self.multi.add(ProgressBar::new(tamanho_bytes.max(1)));
        let estilo = ProgressStyle::with_template(
            "  {spinner:.green} {prefix:.bold} [{elapsed_precise}] [{bar:28.green/white}] {bytes}/{total_bytes} ({bytes_per_sec}) {msg}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=> ");
        pb.set_style(estilo);
        pb.set_prefix(truncar_nome(nome_arquivo, 28));
        pb.set_message("analisando...");
        pb.enable_steady_tick(Duration::from_millis(100));

        BarraVideo {
            barra: pb,
            tamanho_total: tamanho_bytes,
        }
    }

    /// Incrementa a barra geral do lote ao finalizar um arquivo.
    pub fn concluir_video_no_lote(&self) {
        if let Some(pb) = &self.barra_geral {
            pb.inc(1);
        }
    }

    /// Finaliza a barra geral do lote.
    pub fn finalizar(&self, mensagem: &str) {
        if let Some(pb) = &self.barra_geral {
            pb.finish_with_message(mensagem.to_string());
        }
    }
}

/// Barra de progresso individual associada ao processamento de um único vídeo.
pub struct BarraVideo {
    barra: ProgressBar,
    tamanho_total: u64,
}

impl BarraVideo {
    /// Atualiza a mensagem de status da etapa atual.
    pub fn definir_mensagem(&self, mensagem: impl Into<String>) {
        self.barra.set_message(mensagem.into());
    }

    /// Atualiza o progresso com base na fração temporal (`0.0..=1.0`) concluída pelo FFmpeg.
    pub fn atualizar_fracao(&self, fracao: f64) {
        let clamped = fracao.clamp(0.0, 1.0);
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let bytes_estimados = (self.tamanho_total as f64 * clamped).round() as u64;
        self.barra
            .set_position(bytes_estimados.min(self.tamanho_total));
    }

    /// Marca a barra como concluída com sucesso.
    pub fn concluir_sucesso(&self, mensagem: impl Into<String>) {
        self.barra.set_position(self.tamanho_total);
        self.barra.finish_with_message(mensagem.into());
    }

    /// Marca a barra como finalizada (ex: arquivo ignorado ou falha).
    pub fn finalizar_com_mensagem(&self, mensagem: impl Into<String>) {
        self.barra.finish_with_message(mensagem.into());
    }
}

/// Formata um tamanho em bytes de forma legível usando base decimal (1 MB = 1.000.000 bytes),
/// alinhada ao limite de 10.000.000 bytes.
pub fn formatar_tamanho_bytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let valor = bytes as f64;
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB ({bytes} bytes)", valor / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.2} MB ({bytes} bytes)", valor / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.2} KB ({bytes} bytes)", valor / 1_000.0)
    } else {
        format!("{bytes} bytes")
    }
}

/// Formata a velocidade média de processamento em MB/s.
pub fn formatar_velocidade(bytes: u64, duracao: Duration) -> String {
    let segundos = duracao.as_secs_f64();
    if segundos <= 0.000_1 {
        return "N/A".to_string();
    }
    #[allow(clippy::cast_precision_loss)]
    let mb_por_segundo = (bytes as f64 / 1_000_000.0) / segundos;
    format!("{mb_por_segundo:.2} MB/s")
}

/// Trunca um nome longo preservando legibilidade na barra de progresso.
fn truncar_nome(nome: &str, max_chars: usize) -> String {
    let contagem = nome.chars().count();
    if contagem <= max_chars {
        nome.to_string()
    } else if max_chars > 3 {
        let prefixo: String = nome.chars().take(max_chars - 3).collect();
        format!("{prefixo}...")
    } else {
        nome.chars().take(max_chars).collect()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn formata_tamanhos_em_base_decimal() {
        assert_eq!(
            formatar_tamanho_bytes(10_000_000),
            "10.00 MB (10000000 bytes)"
        );
        assert_eq!(formatar_tamanho_bytes(9_800_000), "9.80 MB (9800000 bytes)");
        assert_eq!(formatar_tamanho_bytes(500), "500 bytes");
    }

    #[test]
    fn trunca_nomes_longos_com_seguranca_unicode() {
        assert_eq!(
            truncar_nome("vídeo_muito_longo_ação_teste.mp4", 12),
            "vídeo_mui..."
        );
        assert_eq!(truncar_nome("curto.mp4", 20), "curto.mp4");
    }
}
