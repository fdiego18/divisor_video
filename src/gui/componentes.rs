//! Componentes visuais reutilizáveis, paleta de cores adaptativa, badges de status
//! e formatadores legíveis em português brasileiro.

use std::time::Duration;

use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Ui, Vec2};

use crate::eventos::EstadoItemFila;
use crate::notificacoes::{CategoriaNotificacao, NotificacaoVisual};

/// Paleta semântica derivada do modo claro/escuro atual do `egui`.
#[derive(Debug, Clone, Copy)]
pub struct PaletaCores {
    pub destaque: Color32,
    pub sucesso: Color32,
    pub aviso: Color32,
    pub erro: Color32,
    pub neutro: Color32,
    pub cartao_fundo: Color32,
    pub cartao_borda: Color32,
    pub texto_secundario: Color32,
}

impl PaletaCores {
    /// Retorna a paleta calibrada para contraste alto e aparência moderna no tema ativo.
    pub fn para_tema(escuro: bool) -> Self {
        if escuro {
            Self {
                destaque: Color32::from_rgb(56, 189, 248),
                sucesso: Color32::from_rgb(52, 211, 153),
                aviso: Color32::from_rgb(251, 191, 36),
                erro: Color32::from_rgb(248, 113, 113),
                neutro: Color32::from_rgb(148, 163, 184),
                cartao_fundo: Color32::from_rgb(30, 34, 42),
                cartao_borda: Color32::from_rgb(52, 59, 72),
                texto_secundario: Color32::from_rgb(160, 170, 186),
            }
        } else {
            Self {
                destaque: Color32::from_rgb(2, 132, 199),
                sucesso: Color32::from_rgb(5, 150, 105),
                aviso: Color32::from_rgb(217, 119, 6),
                erro: Color32::from_rgb(220, 38, 38),
                neutro: Color32::from_rgb(100, 116, 139),
                cartao_fundo: Color32::from_rgb(248, 250, 252),
                cartao_borda: Color32::from_rgb(220, 226, 234),
                texto_secundario: Color32::from_rgb(86, 98, 116),
            }
        }
    }
}

/// Retorna a cor semântica associada a um estado de item na fila.
pub fn cor_estado(estado: EstadoItemFila, paleta: PaletaCores) -> Color32 {
    match estado {
        EstadoItemFila::AguardandoAnalise | EstadoItemFila::AguardandoProcessamento => {
            paleta.neutro
        }
        EstadoItemFila::Analisando | EstadoItemFila::Processando | EstadoItemFila::Validando => {
            paleta.destaque
        }
        EstadoItemFila::Concluido => paleta.sucesso,
        EstadoItemFila::Ignorado | EstadoItemFila::Cancelado => paleta.aviso,
        EstadoItemFila::Erro => paleta.erro,
    }
}

/// Desenha um badge arredondado compacto indicando o status atual.
pub fn desenhar_badge_status(ui: &mut Ui, rotulo: &str, cor: Color32) {
    let fundo = Color32::from_rgba_unmultiplied(cor.r(), cor.g(), cor.b(), 30);
    Frame::new()
        .fill(fundo)
        .stroke(Stroke::new(1.0, cor))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(rotulo).size(11.5).strong().color(cor));
        });
}

/// Cria um container de cartão padronizado com bordas arredondadas suaves.
pub fn frame_cartao(paleta: PaletaCores) -> Frame {
    Frame::new()
        .fill(paleta.cartao_fundo)
        .stroke(Stroke::new(1.0, paleta.cartao_borda))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
}

/// Desenha cabeçalho de seção com título e subtítulo opcional.
pub fn desenhar_cabecalho_secao(
    ui: &mut Ui,
    titulo: &str,
    subtitulo: Option<&str>,
    paleta: PaletaCores,
) {
    ui.label(RichText::new(titulo).size(20.0).strong());
    if let Some(sub) = subtitulo {
        ui.label(RichText::new(sub).size(13.0).color(paleta.texto_secundario));
    }
    ui.add_space(6.0);
}

/// Formata tamanho em bytes usando unidades decimais (`1 MB = 1.000.000 bytes`).
pub fn formatar_bytes_decimal(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let v = bytes as f64;
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", v / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.2} MB", v / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", v / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

/// Formata uma duração em segundos como `HH:MM:SS` ou `MM:SS`.
pub fn formatar_duracao_segundos(segundos: f64) -> String {
    if !segundos.is_finite() || segundos < 0.0 {
        return "—".to_string();
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let total = segundos.round() as u64;
    let horas = total / 3600;
    let minutos = (total % 3600) / 60;
    let segs = total % 60;
    if horas > 0 {
        format!("{horas:02}:{minutos:02}:{segs:02}")
    } else {
        format!("{minutos:02}:{segs:02}")
    }
}

/// Formata uma `Duration` de processamento de forma compacta (`1.4s`, `02:15`, etc.).
pub fn formatar_tempo_decorrido(duracao: Duration) -> String {
    let segs = duracao.as_secs_f64();
    if segs < 60.0 {
        format!("{segs:.1}s")
    } else {
        formatar_duracao_segundos(segs)
    }
}

/// Renderiza os toasts ativos no canto inferior direito sem bloquear a janela principal.
pub fn desenhar_toasts(
    ctx: &egui::Context,
    notificacoes: &[NotificacaoVisual],
    paleta: PaletaCores,
) -> Option<u64> {
    if notificacoes.is_empty() {
        return None;
    }

    let mut descartar_id = None;

    egui::Area::new(egui::Id::new("area_toasts_notificacoes"))
        .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -16.0))
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_max_width(360.0);
            for notif in notificacoes {
                let cor = match notif.categoria {
                    CategoriaNotificacao::ProcessamentoConcluido
                    | CategoriaNotificacao::FilaFinalizada => paleta.sucesso,
                    CategoriaNotificacao::ProcessamentoComFalhas
                    | CategoriaNotificacao::FaltaEspacoDisco
                    | CategoriaNotificacao::DependenciasIndisponiveis => paleta.erro,
                    CategoriaNotificacao::Cancelamento => paleta.aviso,
                    CategoriaNotificacao::Informativa => paleta.destaque,
                };

                Frame::new()
                    .fill(paleta.cartao_fundo)
                    .stroke(Stroke::new(1.5, cor))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&notif.titulo).strong().color(cor));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("✕").clicked() {
                                        descartar_id = Some(notif.id);
                                    }
                                },
                            );
                        });
                        ui.label(RichText::new(&notif.mensagem).size(12.0));
                    });
                ui.add_space(6.0);
            }
        });

    descartar_id
}
