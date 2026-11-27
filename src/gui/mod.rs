//! Camada gráfica desktop baseada em `egui` e `eframe`.
//!
//! Mantém toda a lógica de processamento desacoplada nas threads de fundo (`fila_processamento`),
//! aplicando tema claro/escuro/automático do sistema, persistência das dimensões da janela,
//! arrastar e soltar global e interface minimalista em português brasileiro.

pub mod aba_configuracoes;
pub mod aba_historico;
pub mod aba_inicio;
pub mod aba_sobre;
pub mod componentes;
pub mod estado;

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    Color32, CornerRadius, Margin, Pos2, Rect, RichText, Sense, Stroke, ThemePreference, Ui, Vec2,
    ViewportBuilder,
};

use crate::configuracoes_persistentes::{ConfiguracoesPersistentes, TemaPreferido};
use crate::gui::aba_configuracoes::renderizar_aba_configuracoes;
use crate::gui::aba_historico::renderizar_aba_historico;
use crate::gui::aba_inicio::renderizar_aba_inicio;
use crate::gui::aba_sobre::renderizar_aba_sobre;
use crate::gui::componentes::{PaletaCores, desenhar_toasts};
use crate::gui::estado::{AbaAtiva, EstadoAplicacao};

/// Aplicativo principal desktop implementando `eframe::App`.
pub struct AplicativoDesktop {
    pub estado: EstadoAplicacao,
    ultimo_salvamento_janela: Instant,
}

impl AplicativoDesktop {
    /// Constrói a instância do aplicativo desktop vinculando o callback de repaint do `egui`.
    pub fn novo(cc: &eframe::CreationContext<'_>) -> Self {
        configurar_estilo_base(&cc.egui_ctx);

        let ctx_repaint = cc.egui_ctx.clone();
        let acordar_ui: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            ctx_repaint.request_repaint();
        });

        let estado = EstadoAplicacao::novo(acordar_ui);
        aplicar_tema(&cc.egui_ctx, estado.configuracoes.tema);

        Self {
            estado,
            ultimo_salvamento_janela: Instant::now(),
        }
    }

    /// Captura dimensões/posição atuais da janela e persiste se houver alteração relevante.
    fn atualizar_dimensoes_janela(&mut self, ctx: &egui::Context, forcar_salvar: bool) {
        let info = ctx.input(|i| i.viewport().clone());
        let mut alterou = false;

        if let Some(maximizada) = info.maximized
            && self.estado.configuracoes.janela.maximizada != maximizada
        {
            self.estado.configuracoes.janela.maximizada = maximizada;
            alterou = true;
        }

        if !self.estado.configuracoes.janela.maximizada
            && let Some(rect) = info.inner_rect
        {
            let w = rect.width();
            let h = rect.height();
            if w >= 600.0
                && h >= 400.0
                && ((self.estado.configuracoes.janela.largura - w).abs() > 2.0
                    || (self.estado.configuracoes.janela.altura - h).abs() > 2.0)
            {
                self.estado.configuracoes.janela.largura = w;
                self.estado.configuracoes.janela.altura = h;
                alterou = true;
            }
        }

        if let Some(outer) = info.outer_rect {
            let px = outer.min.x;
            let py = outer.min.y;
            if px.is_finite()
                && py.is_finite()
                && (self.estado.configuracoes.janela.pos_x != Some(px)
                    || self.estado.configuracoes.janela.pos_y != Some(py))
            {
                self.estado.configuracoes.janela.pos_x = Some(px);
                self.estado.configuracoes.janela.pos_y = Some(py);
                alterou = true;
            }
        }

        if (alterou && self.ultimo_salvamento_janela.elapsed() >= Duration::from_secs(2))
            || forcar_salvar
        {
            let _ = self.estado.configuracoes.salvar_padrao();
            self.ultimo_salvamento_janela = Instant::now();
        }
    }

    /// Renderiza o menu lateral minimalista com as 4 áreas principais e ícones vetoriais.
    fn renderizar_barra_lateral(&mut self, ui: &mut Ui, paleta: PaletaCores) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            desenhar_icone_logo(ui, paleta.destaque);
            ui.vertical(|ui| {
                ui.label(RichText::new("Divisor de Vídeo").size(15.5).strong());
                ui.label(
                    RichText::new("Qualidade original preservada")
                        .size(11.0)
                        .color(paleta.texto_secundario),
                );
            });
        });

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);

        for aba in [
            AbaAtiva::Inicio,
            AbaAtiva::Historico,
            AbaAtiva::Configuracoes,
            AbaAtiva::SobreAjuda,
        ] {
            let selecionada = self.estado.aba_ativa == aba;
            if botao_navegacao_lateral(ui, aba, selecionada, paleta) {
                self.estado.aba_ativa = aba;
                if aba == AbaAtiva::Historico {
                    self.estado.recarregar_historico();
                }
            }
            ui.add_space(4.0);
        }

        // Rodapé informativo compacto na barra lateral
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .size(11.0)
                    .color(paleta.texto_secundario),
            );
            let status_ff = if self.estado.diagnostico.tudo_disponivel() {
                RichText::new("● FFmpeg pronto")
                    .size(11.5)
                    .color(paleta.sucesso)
            } else {
                RichText::new("● FFmpeg ausente")
                    .size(11.5)
                    .color(paleta.erro)
            };
            ui.label(status_ff);
            ui.separator();
        });
    }
}

impl eframe::App for AplicativoDesktop {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Processa eventos das threads de fundo mesmo com a janela minimizada.
        self.estado.processar_eventos_pendentes(Some(ctx));

        // Mantém tempo decorrido e expiração de toasts atualizados sem polling agressivo.
        if self.estado.motor.esta_processando() || !self.estado.notificacoes.ativas().is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        aplicar_tema(&ctx, self.estado.configuracoes.tema);
        self.atualizar_dimensoes_janela(&ctx, false);

        // Captura global de arquivos ou pastas arrastados e soltos sobre a janela.
        let arquivos_soltos: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !arquivos_soltos.is_empty() {
            let recursivo = self.estado.configuracoes.busca_recursiva;
            self.estado.aba_ativa = AbaAtiva::Inicio;
            self.estado.adicionar_caminhos(arquivos_soltos, recursivo);
        }

        let escuro = ctx.global_style().visuals.dark_mode;
        let paleta = PaletaCores::para_tema(escuro);

        egui::Panel::left("painel_navegacao_lateral")
            .resizable(false)
            .exact_size(205.0)
            .show(ui, |ui| {
                self.renderizar_barra_lateral(ui, paleta);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.global_style()).inner_margin(Margin::same(18)))
            .show(ui, |ui| match self.estado.aba_ativa {
                AbaAtiva::Inicio => {
                    egui::ScrollArea::vertical()
                        .id_salt("scroll_inicio")
                        .auto_shrink([false, false])
                        .show(ui, |ui| renderizar_aba_inicio(ui, &mut self.estado, paleta));
                }
                AbaAtiva::Historico => renderizar_aba_historico(ui, &mut self.estado, paleta),
                AbaAtiva::Configuracoes => {
                    renderizar_aba_configuracoes(ui, &mut self.estado, paleta);
                }
                AbaAtiva::SobreAjuda => renderizar_aba_sobre(ui, &self.estado, paleta),
            });

        // Toasts flutuantes de notificação
        if let Some(id_descartar) = desenhar_toasts(&ctx, self.estado.notificacoes.ativas(), paleta)
        {
            self.estado.notificacoes.descartar(id_descartar);
        }
    }

    fn on_exit(&mut self) {
        self.estado.motor.cancelar_tudo();
        let _ = self.estado.configuracoes.salvar_padrao();
    }
}

/// Inicia a janela desktop nativa do `divisor_video` restaurando as dimensões persistidas.
pub fn executar_aplicativo_desktop() -> eframe::Result {
    let cfg = ConfiguracoesPersistentes::carregar_padrao();

    let mut viewport = ViewportBuilder::default()
        .with_title("Divisor de Vídeo — Stream Copy sem Perda")
        .with_inner_size([cfg.janela.largura, cfg.janela.altura])
        .with_min_inner_size([840.0, 560.0])
        .with_drag_and_drop(true)
        .with_maximized(cfg.janela.maximizada);

    if let (Some(px), Some(py)) = (cfg.janela.pos_x, cfg.janela.pos_y) {
        viewport = viewport.with_position([px, py]);
    }

    let opcoes = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "divisor_video_gui",
        opcoes,
        Box::new(|cc| Ok(Box::new(AplicativoDesktop::novo(cc)))),
    )
}

/// Aplica espaçamentos e tipografia legíveis para telas padrão e HiDPI.
fn configurar_estilo_base(ctx: &egui::Context) {
    ctx.all_styles_mut(|estilo| {
        estilo.spacing.item_spacing = Vec2::new(8.0, 6.0);
        estilo.spacing.button_padding = Vec2::new(10.0, 5.0);
        estilo.visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
        estilo.visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
        estilo.visuals.widgets.active.corner_radius = CornerRadius::same(6);
    });
}

/// Sincroniza o tema escolhido (`Sistema`, `Escuro` ou `Claro`) com o `egui`.
fn aplicar_tema(ctx: &egui::Context, tema: TemaPreferido) {
    let pref = match tema {
        TemaPreferido::Sistema => ThemePreference::System,
        TemaPreferido::Escuro => ThemePreference::Dark,
        TemaPreferido::Claro => ThemePreference::Light,
    };
    ctx.set_theme(pref);
}

/// Botão de navegação lateral com ícone vetorial nítido em qualquer resolução HiDPI.
fn botao_navegacao_lateral(
    ui: &mut Ui,
    aba: AbaAtiva,
    selecionada: bool,
    paleta: PaletaCores,
) -> bool {
    let largura = ui.available_width();
    let altura = 38.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(largura, altura), Sense::click());

    let fundo = if selecionada {
        Color32::from_rgba_unmultiplied(
            paleta.destaque.r(),
            paleta.destaque.g(),
            paleta.destaque.b(),
            36,
        )
    } else if resp.hovered() {
        Color32::from_rgba_unmultiplied(128, 128, 128, 22)
    } else {
        Color32::TRANSPARENT
    };

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), fundo);

    if selecionada {
        let barra = Rect::from_min_size(
            rect.min + Vec2::new(2.0, 7.0),
            Vec2::new(3.0, altura - 14.0),
        );
        painter.rect_filled(barra, CornerRadius::same(2), paleta.destaque);
    }

    let cor_conteudo = if selecionada {
        paleta.destaque
    } else {
        ui.visuals().text_color()
    };

    let centro_icone = Pos2::new(rect.min.x + 22.0, rect.center().y);
    desenhar_icone_aba(painter, aba, centro_icone, cor_conteudo);

    painter.text(
        Pos2::new(rect.min.x + 40.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        aba.rotulo(),
        egui::FontId::proportional(14.0),
        cor_conteudo,
    );

    resp.clicked()
}

/// Desenha o ícone vetorial do aplicativo no topo da barra lateral.
fn desenhar_icone_logo(ui: &mut Ui, cor: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(7),
        Color32::from_rgba_unmultiplied(cor.r(), cor.g(), cor.b(), 32),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(7),
        Stroke::new(1.5, cor),
        egui::StrokeKind::Inside,
    );
    // Dois segmentos representando divisão limpa de vídeo
    let esq = Rect::from_min_max(
        rect.min + Vec2::new(6.0, 8.0),
        Pos2::new(rect.center().x - 2.0, rect.max.y - 8.0),
    );
    let dir = Rect::from_min_max(
        Pos2::new(rect.center().x + 2.0, rect.min.y + 8.0),
        rect.max - Vec2::new(6.0, 8.0),
    );
    painter.rect_filled(esq, CornerRadius::same(2), cor);
    painter.rect_filled(dir, CornerRadius::same(2), cor);
}

/// Desenha ícones vetoriais minimalistas e consistentes para cada aba da barra lateral.
fn desenhar_icone_aba(painter: &egui::Painter, aba: AbaAtiva, c: Pos2, cor: Color32) {
    let traco = Stroke::new(1.6, cor);
    match aba {
        AbaAtiva::Inicio => {
            // Ícone de claquete/vídeo dividido
            let r = Rect::from_center_size(c, Vec2::new(14.0, 11.0));
            painter.rect_stroke(r, CornerRadius::same(2), traco, egui::StrokeKind::Middle);
            painter.line_segment([Pos2::new(c.x, r.min.y), Pos2::new(c.x, r.max.y)], traco);
        }
        AbaAtiva::Historico => {
            // Relógio minimalista
            painter.circle_stroke(c, 6.5, traco);
            painter.line_segment([c, c + Vec2::new(0.0, -3.5)], traco);
            painter.line_segment([c, c + Vec2::new(3.0, 1.5)], traco);
        }
        AbaAtiva::Configuracoes => {
            // Controles deslizantes
            for dy in [-4.0_f32, 0.0, 4.0] {
                painter.line_segment(
                    [
                        Pos2::new(c.x - 6.5, c.y + dy),
                        Pos2::new(c.x + 6.5, c.y + dy),
                    ],
                    traco,
                );
            }
            painter.circle_filled(Pos2::new(c.x - 2.5, c.y - 4.0), 2.0, cor);
            painter.circle_filled(Pos2::new(c.x + 2.5, c.y), 2.0, cor);
            painter.circle_filled(Pos2::new(c.x - 1.0, c.y + 4.0), 2.0, cor);
        }
        AbaAtiva::SobreAjuda => {
            // Círculo de informação
            painter.circle_stroke(c, 6.5, traco);
            painter.circle_filled(Pos2::new(c.x, c.y - 2.8), 1.1, cor);
            painter.line_segment(
                [Pos2::new(c.x, c.y - 0.3), Pos2::new(c.x, c.y + 3.5)],
                traco,
            );
        }
    }
}
