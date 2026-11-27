//! Aba principal (Início): seleção/arrastar-e-soltar de vídeos, configuração rápida
//! do limite e diretório de saída, fila visual de processamento e acesso aos segmentos gerados.

use std::path::PathBuf;

use egui::{
    Color32, ComboBox, CornerRadius, Frame, Margin, ProgressBar, RichText, Stroke, Ui, Vec2,
};

use crate::configuracoes_persistentes::LimitePredefinido;
use crate::eventos::EstadoItemFila;
use crate::gui::componentes::{
    PaletaCores, cor_estado, desenhar_badge_status, desenhar_cabecalho_secao,
    formatar_bytes_decimal, formatar_duracao_segundos, formatar_tempo_decorrido, frame_cartao,
};
use crate::gui::estado::{AbaAtiva, EstadoAplicacao};
use crate::integracao_so::abrir_no_sistema;
use crate::notificacoes::CategoriaNotificacao;

/// Ações coletadas durante a renderização da fila e aplicadas fora do empréstimo mutável da lista.
enum AcaoItem {
    Remover(u64),
    MoverCima(u64),
    MoverBaixo(u64),
    Cancelar(u64),
    Reprocessar(u64),
    AlternarExpandido(u64),
    AbrirCaminho(PathBuf),
    CopiarCaminho(String),
}

/// Renderiza todo o conteúdo da aba **Início**.
pub fn renderizar_aba_inicio(ui: &mut Ui, estado: &mut EstadoAplicacao, paleta: PaletaCores) {
    desenhar_cabecalho_secao(
        ui,
        "Divida seus vídeos em poucos passos",
        Some(
            "Escolha os vídeos, defina o tamanho das partes e comece. A qualidade original é preservada.",
        ),
        paleta,
    );

    // Alerta destacado se FFmpeg/FFprobe não estiverem disponíveis.
    if !estado.diagnostico.tudo_disponivel() {
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                paleta.erro.r(),
                paleta.erro.g(),
                paleta.erro.b(),
                26,
            ))
            .stroke(Stroke::new(1.2, paleta.erro))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(
                            "Dependências ausentes: FFmpeg e/ou FFprobe não foram encontrados.",
                        )
                        .strong()
                        .color(paleta.erro),
                    );
                    if ui.button("Configurar agora").clicked() {
                        estado.aba_ativa = AbaAtiva::Configuracoes;
                    }
                });
            });
        ui.add_space(8.0);
    }

    ui.label(RichText::new("1. Escolha os vídeos").strong());
    // Zona de arrastar e soltar + botões de seleção nativa.
    renderizar_zona_selecao(ui, estado, paleta);
    ui.add_space(10.0);

    // Painel compacto de configuração rápida (limite, margem, pasta de saída e concorrência).
    ui.label(RichText::new("2. Defina o tamanho e onde salvar").strong());
    renderizar_painel_parametros(ui, estado, paleta);
    ui.add_space(10.0);

    // Controles de execução + barra de progresso global + resumo da última fila.
    ui.label(RichText::new("3. Divida e acompanhe").strong());
    renderizar_barra_controles_fila(ui, estado, paleta);
    ui.add_space(10.0);

    // Lista visual dos vídeos na fila.
    renderizar_lista_fila(ui, estado, paleta);
}

/// Zona interativa de arrastar/soltar arquivos ou diretórios e seleção via diálogo nativo.
fn renderizar_zona_selecao(ui: &mut Ui, estado: &mut EstadoAplicacao, paleta: PaletaCores) {
    let arrastando = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    let cor_borda = if arrastando {
        paleta.destaque
    } else {
        paleta.cartao_borda
    };
    let fundo = if arrastando {
        Color32::from_rgba_unmultiplied(
            paleta.destaque.r(),
            paleta.destaque.g(),
            paleta.destaque.b(),
            28,
        )
    } else {
        paleta.cartao_fundo
    };

    Frame::new()
        .fill(fundo)
        .stroke(Stroke::new(if arrastando { 2.0 } else { 1.0 }, cor_borda))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Selecionar vídeos...").clicked()
                    && let Some(arquivos) = rfd::FileDialog::new()
                        .set_title("Selecionar arquivos de vídeo")
                        .add_filter(
                            "Vídeos compatíveis",
                            &[
                                "mp4", "mkv", "mov", "avi", "webm", "m4v", "flv", "ts", "m2ts",
                                "mts", "mpeg", "mpg", "wmv", "ogv", "3gp",
                            ],
                        )
                        .pick_files()
                {
                    estado.adicionar_caminhos(arquivos, false);
                }

                if ui.button("Selecionar pasta...").clicked()
                    && let Some(pasta) = rfd::FileDialog::new()
                        .set_title("Selecionar pasta contendo vídeos")
                        .pick_folder()
                {
                    let rec = estado.configuracoes.busca_recursiva;
                    estado.adicionar_caminhos(vec![pasta], rec);
                }

                let mut recursivo = estado.configuracoes.busca_recursiva;
                if ui
                    .checkbox(&mut recursivo, "Incluir subpastas")
                    .changed()
                {
                    estado.configuracoes.busca_recursiva = recursivo;
                    estado.salvar_configuracoes();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let tem_itens = !estado.itens_fila.is_empty();
                    if ui
                        .add_enabled(tem_itens, egui::Button::new("Limpar fila"))
                        .clicked()
                    {
                        estado.limpar_fila();
                    }
                });
            });

            ui.add_space(4.0);
            let dica = if arrastando {
                "Solte os arquivos ou pastas aqui para adicionar à fila!"
            } else {
                "Você também pode arrastar vídeos ou pastas para esta janela. Aceita MP4, MKV, MOV, AVI e outros formatos."
            };
            ui.label(
                RichText::new(dica)
                    .size(12.0)
                    .color(if arrastando {
                        paleta.destaque
                    } else {
                        paleta.texto_secundario
                    }),
            );
        });
}

/// Painel rápido de limite por segmento, margem de segurança, pasta de saída e concorrência.
fn renderizar_painel_parametros(ui: &mut Ui, estado: &mut EstadoAplicacao, paleta: PaletaCores) {
    frame_cartao(paleta).show(ui, |ui| {
        ui.set_width(ui.available_width());

        // Linha 1: Limite de tamanho + valor personalizado + margem automática
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Tamanho máximo de cada parte:").strong());
            let mut selecionado = estado.configuracoes.limite_predefinido;
            ComboBox::from_id_salt("combo_limite_inicio")
                .selected_text(selecionado.rotulo())
                .width(155.0)
                .show_ui(ui, |ui| {
                    for &opcao in LimitePredefinido::TODAS {
                        ui.selectable_value(&mut selecionado, opcao, opcao.rotulo());
                    }
                });
            if selecionado != estado.configuracoes.limite_predefinido {
                estado.configuracoes.limite_predefinido = selecionado;
                estado.salvar_configuracoes();
            }

            if estado.configuracoes.limite_predefinido == LimitePredefinido::Personalizado {
                ui.label("Valor (MB):");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut estado.campo_limite_personalizado_mb)
                        .desired_width(75.0)
                        .hint_text("ex: 15"),
                );
                let aplicar = ui.button("Aplicar").clicked();
                if (resp.lost_focus() || aplicar)
                    && !estado.campo_limite_personalizado_mb.trim().is_empty()
                {
                    estado.aplicar_limite_personalizado();
                }
                let valor = estado.campo_limite_personalizado_mb.trim().replace(',', ".").parse::<f64>();
                if !matches!(valor, Ok(mb) if mb.is_finite() && (0.5..=100_000.0).contains(&mb)) {
                    ui.colored_label(paleta.erro, "Digite um valor entre 0,5 e 100.000 MB. O último limite válido continua em uso.");
                }
            }

            ui.label(
                RichText::new("Cada parte ficará dentro do limite escolhido.")
                    .size(12.0)
                    .color(paleta.texto_secundario),
            );
        });

        ui.add_space(6.0);

        // Linha 2: Diretório de saída + espaço livre + concorrência + sobrescrita
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Salvar em:").strong());
            let dir_str = estado.configuracoes.diretorio_saida.display().to_string();
            ui.label(RichText::new(&dir_str).monospace().size(12.0));

            if ui.small_button("Alterar pasta...").clicked()
                && let Some(nova_pasta) = rfd::FileDialog::new()
                    .set_title("Selecionar diretório de saída")
                    .pick_folder()
            {
                estado.configuracoes.diretorio_saida = nova_pasta;
                estado.salvar_configuracoes();
            }

            if ui.small_button("Abrir pasta").clicked() {
                if !estado.configuracoes.diretorio_saida.exists() {
                    let _ = std::fs::create_dir_all(&estado.configuracoes.diretorio_saida);
                }
                if let Err(erro) = abrir_no_sistema(&estado.configuracoes.diretorio_saida) {
                    estado.notificacoes.emitir(
                        CategoriaNotificacao::Informativa,
                        "Abertura de pasta",
                        erro.to_string(),
                        &estado.configuracoes.notificacoes,
                    );
                }
            }

            if let Some(livre) = estado.espaco_livre_saida_bytes {
                ui.label(
                    RichText::new(format!("• Livre: {}", formatar_bytes_decimal(livre)))
                        .size(12.0)
                        .color(paleta.texto_secundario),
                );
            }
        });

        ui.add_space(6.0);
        ui.collapsing("Opções avançadas", |ui| {
            ui.label("A margem de segurança é aplicada automaticamente ao tamanho das partes.");
            ui.label(format!(
                "Tamanho alvo: {} • Margem: {}",
                formatar_bytes_decimal(estado.configuracoes.tamanho_alvo_efetivo_bytes()),
                formatar_bytes_decimal(estado.configuracoes.margem_seguranca_bytes()),
            ));
            ui.label("Vídeos ao mesmo tempo:");
            let mut conc = estado.configuracoes.concorrencia;
            if ui
                .add(egui::DragValue::new(&mut conc).range(1..=16).speed(0.1))
                .changed()
            {
                estado.configuracoes.concorrencia = conc.clamp(1, 16);
                estado.salvar_configuracoes();
            }

            let mut sobrescrever = estado.configuracoes.permitir_sobrescrita;
            if ui
                .checkbox(&mut sobrescrever, "Substituir arquivos de saída existentes")
                .changed()
            {
                estado.configuracoes.permitir_sobrescrita = sobrescrever;
                estado.salvar_configuracoes();
            }
        });
    });
}

/// Barra de ação principal da fila (Iniciar, Cancelar, Reprocessar) e progresso global.
fn renderizar_barra_controles_fila(ui: &mut Ui, estado: &mut EstadoAplicacao, paleta: PaletaCores) {
    let processando = estado.motor.esta_processando();
    let prontos = estado.quantidade_prontos();
    let reprocessaveis = estado.quantidade_reprocessaveis();
    let ferramentas_ok = estado.diagnostico.tudo_disponivel();

    ui.horizontal_wrapped(|ui| {
        let rotulo_iniciar = if processando {
            format!("Adicionar à execução ({prontos})")
        } else {
            format!("Dividir vídeos ({prontos})")
        };

        let botao_iniciar =
            egui::Button::new(RichText::new(rotulo_iniciar).strong().color(Color32::WHITE))
                .min_size(Vec2::new(180.0, 38.0))
                .fill(if prontos > 0 && ferramentas_ok {
                    paleta.destaque
                } else {
                    paleta.neutro
                });

        if ui
            .add_enabled(prontos > 0 && ferramentas_ok, botao_iniciar)
            .clicked()
        {
            estado.iniciar_ou_enfileirar_pendentes();
        }

        if ui
            .add_enabled(
                reprocessaveis > 0 && ferramentas_ok,
                egui::Button::new(format!("Reprocessar falhas ({reprocessaveis})")),
            )
            .clicked()
        {
            estado.reprocessar_falhas();
        }

        if processando {
            let botao_cancelar = egui::Button::new(
                RichText::new("Cancelar fila")
                    .strong()
                    .color(Color32::WHITE),
            )
            .fill(paleta.erro);
            if ui.add(botao_cancelar).clicked() {
                estado.cancelar_toda_fila();
            }
        }

        if let Some(inicio) = estado.inicio_fila_atual {
            ui.label(
                RichText::new(format!(
                    "Tempo decorrido: {}",
                    formatar_tempo_decorrido(inicio.elapsed())
                ))
                .size(12.5)
                .color(paleta.texto_secundario),
            );
        }
    });

    let orientacao = if !ferramentas_ok {
        "Configure o FFmpeg e o FFprobe para começar. Use o botão Configurar agora acima."
    } else if estado.itens_fila.is_empty() {
        "Selecione pelo menos um vídeo na etapa 1 para começar."
    } else if estado
        .itens_fila
        .iter()
        .any(|item| item.estado == EstadoItemFila::Analisando)
    {
        "Estamos verificando os vídeos. Eles ficarão disponíveis para divisão após a análise."
    } else if processando {
        "Divisão em andamento. Você pode adicionar mais vídeos enquanto aguarda."
    } else if prontos > 0 {
        "Tudo pronto. Confira a pasta de destino e clique em Dividir vídeos."
    } else {
        "Confira o status de cada vídeo abaixo. Vídeos dentro do limite não precisam ser divididos."
    };
    ui.label(RichText::new(orientacao).color(paleta.texto_secundario));

    if !estado.itens_fila.is_empty() {
        ui.add_space(6.0);
        let prog_global = estado.progresso_global();
        ui.add(
            ProgressBar::new(prog_global)
                .text(format!(
                    "Progresso global: {:.0}% ({} vídeo(s) na fila)",
                    prog_global * 100.0,
                    estado.itens_fila.len()
                ))
                .animate(processando),
        );
    }

    if let Some(resumo) = &estado.ultimo_resumo_fila {
        ui.add_space(6.0);
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                paleta.sucesso.r(),
                paleta.sucesso.g(),
                paleta.sucesso.b(),
                22,
            ))
            .stroke(Stroke::new(1.0, paleta.sucesso))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!(
                        "Resumo final: {} vídeo(s) processado(s) ({} dividido(s), {} ignorado(s)) • {} segmento(s) gerado(s) • {} falha(s) • {} cancelado(s) • Tempo: {} • Velocidade média: {:.1} MB/s",
                        resumo.videos_processados,
                        resumo.videos_divididos,
                        resumo.videos_ignorados,
                        resumo.segmentos_gerados,
                        resumo.falhas,
                        resumo.cancelados,
                        formatar_tempo_decorrido(resumo.tempo_total),
                        resumo.velocidade_media_mb_s
                    ))
                    .size(12.5)
                    .strong(),
                );
            });
    }
}

/// Lista rolável contendo cada vídeo da fila, sua miniatura, metadados técnicos, progresso e segmentos.
fn renderizar_lista_fila(ui: &mut Ui, estado: &mut EstadoAplicacao, paleta: PaletaCores) {
    if estado.itens_fila.is_empty() {
        ui.add_space(36.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Nenhum vídeo na fila")
                    .size(16.0)
                    .strong()
                    .color(paleta.texto_secundario),
            );
            ui.label(
                RichText::new(
                    "Arraste vídeos ou pastas para cá, ou clique em \"Selecionar vídeos...\".",
                )
                .size(13.0)
                .color(paleta.texto_secundario),
            );
        });
        return;
    }

    let limite_max = estado.configuracoes.limite_maximo_efetivo_bytes();
    let limite_alvo = estado.configuracoes.tamanho_alvo_efetivo_bytes();
    let sobrescrita_ativa = estado.configuracoes.permitir_sobrescrita;
    let total_itens = estado.itens_fila.len();
    let mut acoes: Vec<AcaoItem> = Vec::new();

    ui.scope(|ui| {
            for (indice, item) in estado.itens_fila.iter().enumerate() {
                frame_cartao(paleta).show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.horizontal(|ui| {
                        // Coluna 1: Miniatura (ou placeholder)
                        let tamanho_thumb = Vec2::new(112.0, 64.0);
                        if let Some(tex) = estado.texturas_miniaturas.get(&item.id) {
                            ui.add(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(tamanho_thumb)
                                    .corner_radius(CornerRadius::same(6)),
                            );
                        } else {
                            Frame::new()
                                .fill(Color32::from_rgba_unmultiplied(128, 128, 128, 30))
                                .corner_radius(CornerRadius::same(6))
                                .show(ui, |ui| {
                                    ui.allocate_ui_with_layout(
                                        tamanho_thumb,
                                        egui::Layout::centered_and_justified(
                                            egui::Direction::TopDown,
                                        ),
                                        |ui| {
                                            ui.label(
                                                RichText::new(if item.estado == EstadoItemFila::Analisando {
                                                    "..."
                                                } else {
                                                    "VÍDEO"
                                                })
                                                .size(11.0)
                                                .color(paleta.texto_secundario),
                                            );
                                        },
                                    );
                                });
                        }

                        ui.add_space(6.0);

                        // Coluna 2: Informações principais, metadados técnicos e progresso
                        ui.vertical(|ui| {
                            // Título + Badge + Botões de ação rápida
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(&item.nome_arquivo).strong().size(14.0));
                                desenhar_badge_status(
                                    ui,
                                    item.estado.rotulo(),
                                    cor_estado(item.estado, paleta),
                                );

                                if item.conflito_saida_existente
                                    && !sobrescrita_ativa
                                    && !matches!(item.estado, EstadoItemFila::Concluido)
                                {
                                    desenhar_badge_status(
                                        ui,
                                        "Saída já existe",
                                        paleta.aviso,
                                    );
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if item.estado.em_andamento() {
                                            if ui
                                                .small_button(
                                                    RichText::new("Cancelar").color(paleta.erro),
                                                )
                                                .clicked()
                                            {
                                                acoes.push(AcaoItem::Cancelar(item.id));
                                            }
                                        } else {
                                            if ui.small_button("Remover").clicked() {
                                                acoes.push(AcaoItem::Remover(item.id));
                                            }
                                            if matches!(
                                                item.estado,
                                                EstadoItemFila::Erro | EstadoItemFila::Cancelado
                                            ) && ui.small_button("Reprocessar").clicked()
                                            {
                                                acoes.push(AcaoItem::Reprocessar(item.id));
                                            }
                                        }

                                        if item.estado.pode_reordenar() {
                                            let pode_descer = indice + 1 < total_itens
                                                && estado.itens_fila[indice + 1]
                                                    .estado
                                                    .pode_reordenar();
                                            if ui
                                                .add_enabled(pode_descer, egui::Button::new("↓").small())
                                                .on_hover_text("Mover para baixo na fila")
                                                .clicked()
                                            {
                                                acoes.push(AcaoItem::MoverBaixo(item.id));
                                            }

                                            let pode_subir = indice > 0
                                                && estado.itens_fila[indice - 1]
                                                    .estado
                                                    .pode_reordenar();
                                            if ui
                                                .add_enabled(pode_subir, egui::Button::new("↑").small())
                                                .on_hover_text("Mover para cima na fila")
                                                .clicked()
                                            {
                                                acoes.push(AcaoItem::MoverCima(item.id));
                                            }
                                        }

                                        if ui
                                            .small_button("Prévia")
                                            .on_hover_text("Abrir vídeo original no reprodutor padrão")
                                            .clicked()
                                        {
                                            acoes.push(AcaoItem::AbrirCaminho(item.caminho.clone()));
                                        }

                                        if ui
                                            .small_button("Copiar caminho")
                                            .clicked()
                                        {
                                            acoes.push(AcaoItem::CopiarCaminho(
                                                item.caminho.display().to_string(),
                                            ));
                                        }
                                    },
                                );
                            });

                            // Caminho completo
                            ui.label(
                                RichText::new(item.caminho.display().to_string())
                                    .size(11.5)
                                    .color(paleta.texto_secundario),
                            );

                            // Linha de metadados técnicos
                            if let Some(resumo) = &item.resumo_tecnico {
                                let partes_est =
                                    resumo.estimar_quantidade_segmentos(limite_max, limite_alvo);
                                let info_partes = if resumo.tamanho_bytes <= limite_max {
                                    "<= limite (será ignorado)".to_string()
                                } else {
                                    format!("~{partes_est} parte(s) estimada(s)")
                                };
                                ui.label(
                                    RichText::new(format!(
                                        "{}  •  {}  •  {}  •  {:.2} FPS  •  Vídeo: {}  •  Áudio: {}  •  {}",
                                        formatar_bytes_decimal(resumo.tamanho_bytes),
                                        formatar_duracao_segundos(resumo.duracao_segundos),
                                        resumo.resolucao_formatada(),
                                        resumo.fps,
                                        resumo.codec_video,
                                        resumo.codec_audio,
                                        info_partes
                                    ))
                                    .size(12.0),
                                );
                            } else {
                                ui.label(
                                    RichText::new(format!(
                                        "Tamanho: {}  •  Aguardando leitura de metadados...",
                                        formatar_bytes_decimal(item.tamanho_bytes())
                                    ))
                                    .size(12.0)
                                    .color(paleta.texto_secundario),
                                );
                            }

                            // Barra de progresso individual (quando em execução ou concluído)
                            if item.estado.em_andamento() || item.progresso > 0.0 {
                                ui.add_space(4.0);
                                let mut info_prog = format!("{:.0}%", item.progresso * 100.0);
                                if !item.detalhe_progresso.is_empty() {
                                    info_prog.push_str(&format!(" — {}", item.detalhe_progresso));
                                }
                                if item.velocidade_mb_s > 0.05 {
                                    info_prog.push_str(&format!(
                                        "  •  {:.1} MB/s",
                                        item.velocidade_mb_s
                                    ));
                                }
                                if item.tempo_decorrido.as_millis() > 0 {
                                    info_prog.push_str(&format!(
                                        "  •  {}",
                                        formatar_tempo_decorrido(item.tempo_decorrido)
                                    ));
                                }
                                if let Some(eta) = item.eta_estimado {
                                    info_prog.push_str(&format!(
                                        "  •  Restante: ~{}",
                                        formatar_tempo_decorrido(eta)
                                    ));
                                }

                                ui.add(
                                    ProgressBar::new(item.progresso)
                                        .text(info_prog)
                                        .animate(item.estado.em_andamento()),
                                );
                            }

                            // Mensagem de erro compreensível
                            if let Some(erro) = &item.mensagem_erro {
                                ui.add_space(4.0);
                                ui.label(RichText::new(erro).size(12.0).color(paleta.erro));
                            }

                            // Ações sobre segmentos gerados
                            if !item.segmentos.is_empty() {
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    let rotulo_seg = if item.expandido {
                                        format!("Ocultar segmentos ({})", item.segmentos.len())
                                    } else {
                                        format!("Ver segmentos gerados ({})", item.segmentos.len())
                                    };
                                    if ui.small_button(rotulo_seg).clicked() {
                                        acoes.push(AcaoItem::AlternarExpandido(item.id));
                                    }
                                    if let Some(dir) = &item.diretorio_saida
                                        && ui.small_button("Abrir pasta de saída").clicked()
                                    {
                                        acoes.push(AcaoItem::AbrirCaminho(dir.clone()));
                                    }
                                });

                                if item.expandido {
                                    ui.add_space(4.0);
                                    for seg in &item.segmentos {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "• {} — {} ({} bytes) — {}",
                                                    seg.nome_arquivo,
                                                    formatar_bytes_decimal(seg.tamanho_bytes),
                                                    seg.tamanho_bytes,
                                                    formatar_duracao_segundos(seg.duracao_segundos)
                                                ))
                                                .size(12.0),
                                            );
                                            if ui.small_button("Abrir").clicked() {
                                                acoes.push(AcaoItem::AbrirCaminho(
                                                    seg.caminho.clone(),
                                                ));
                                            }
                                            if ui.small_button("Copiar caminho").clicked() {
                                                acoes.push(AcaoItem::CopiarCaminho(
                                                    seg.caminho.display().to_string(),
                                                ));
                                            }
                                        });
                                    }
                                }
                            }
                        });
                    });
                });
                ui.add_space(6.0);
            }
        });

    // Aplica ações coletadas fora do loop.
    for acao in acoes {
        match acao {
            AcaoItem::Remover(id) => estado.remover_item(id),
            AcaoItem::MoverCima(id) => estado.mover_item_para_cima(id),
            AcaoItem::MoverBaixo(id) => estado.mover_item_para_baixo(id),
            AcaoItem::Cancelar(id) => estado.cancelar_item(id),
            AcaoItem::Reprocessar(id) => estado.reprocessar_item(id),
            AcaoItem::AlternarExpandido(id) => {
                if let Some(it) = estado.itens_fila.iter_mut().find(|i| i.id == id) {
                    it.expandido = !it.expandido;
                }
            }
            AcaoItem::AbrirCaminho(caminho) => {
                if let Err(erro) = abrir_no_sistema(&caminho) {
                    estado.notificacoes.emitir(
                        CategoriaNotificacao::Informativa,
                        "Não foi possível abrir",
                        erro.to_string(),
                        &estado.configuracoes.notificacoes,
                    );
                }
            }
            AcaoItem::CopiarCaminho(texto) => {
                ui.ctx().copy_text(texto);
                estado.notificacoes.emitir(
                    CategoriaNotificacao::Informativa,
                    "Caminho copiado",
                    "O caminho foi copiado para a área de transferência.",
                    &estado.configuracoes.notificacoes,
                );
            }
        }
    }
}
