//! Aba Histórico: consulta, pesquisa, filtros por data e status, exportação CSV/JSON,
//! reprocessamento e gerenciamento seguro do histórico SQLite local.

use std::path::PathBuf;
use std::time::Duration;

use egui::{ComboBox, RichText, Ui};

use crate::gui::componentes::{
    PaletaCores, desenhar_badge_status, desenhar_cabecalho_secao, formatar_bytes_decimal,
    formatar_duracao_segundos, formatar_tempo_decorrido, frame_cartao,
};
use crate::gui::estado::EstadoAplicacao;
use crate::integracao_so::abrir_no_sistema;
use crate::notificacoes::CategoriaNotificacao;

enum AcaoHistorico {
    AbrirCaminho(PathBuf),
    CopiarTexto(String),
    Reprocessar(PathBuf),
    Excluir(i64),
    AlternarExpandido(i64),
}

/// Renderiza todo o conteúdo da aba **Histórico**.
pub fn renderizar_aba_historico(ui: &mut Ui, estado: &mut EstadoAplicacao, paleta: PaletaCores) {
    desenhar_cabecalho_secao(
        ui,
        "Histórico de Processamentos",
        Some(
            "Consulte, filtre, reprocessar ou exporte o registro local (SQLite). Excluir registros nunca apaga seus vídeos.",
        ),
        paleta,
    );

    if let Some(erro) = &estado.erro_historico {
        ui.label(RichText::new(format!("Erro no histórico: {erro}")).color(paleta.erro));
        ui.add_space(6.0);
    }

    // Barra de filtros e exportação
    let mut recarregar = false;
    frame_cartao(paleta).show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal_wrapped(|ui| {
            ui.label("Pesquisar:");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut estado.filtro_historico.termo_nome)
                        .desired_width(200.0)
                        .hint_text("Nome ou caminho do vídeo..."),
                )
                .changed()
            {
                recarregar = true;
            }

            ui.label("De:");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut estado.filtro_historico.data_inicio)
                        .desired_width(92.0)
                        .hint_text("AAAA-MM-DD"),
                )
                .changed()
            {
                recarregar = true;
            }

            ui.label("Até:");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut estado.filtro_historico.data_fim)
                        .desired_width(92.0)
                        .hint_text("AAAA-MM-DD"),
                )
                .changed()
            {
                recarregar = true;
            }

            ui.label("Status:");
            let texto_status = estado.filtro_historico.status.as_deref().unwrap_or("Todos");
            let mut status_atual = estado.filtro_historico.status.clone();
            ComboBox::from_id_salt("combo_status_historico")
                .selected_text(texto_status)
                .width(120.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut status_atual, None, "Todos");
                    for opcao in ["Concluído", "Ignorado", "Cancelado", "Erro"] {
                        ui.selectable_value(&mut status_atual, Some(opcao.to_string()), opcao);
                    }
                });
            if status_atual != estado.filtro_historico.status {
                estado.filtro_historico.status = status_atual;
                recarregar = true;
            }

            if ui.small_button("Limpar filtros").clicked() {
                estado.filtro_historico = Default::default();
                recarregar = true;
            }
        });

        ui.add_space(6.0);

        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} registro(s) encontrado(s)",
                    estado.registros_historico.len()
                ))
                .size(12.0)
                .color(paleta.texto_secundario),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let tem_registros = !estado.registros_historico.is_empty();

                if ui
                    .add_enabled(
                        tem_registros,
                        egui::Button::new(RichText::new("Limpar histórico").color(paleta.erro)),
                    )
                    .clicked()
                {
                    estado.confirmar_limpeza_historico = true;
                }

                if ui
                    .add_enabled(tem_registros, egui::Button::new("Exportar JSON"))
                    .clicked()
                    && let Some(destino) = rfd::FileDialog::new()
                        .set_title("Exportar histórico em JSON")
                        .set_file_name("historico_divisor_video.json")
                        .add_filter("JSON", &["json"])
                        .save_file()
                    && let Some(repo) = &estado.repositorio_historico
                {
                    match repo.exportar_json(&estado.filtro_historico, &destino) {
                        Ok(qtd) => estado.notificacoes.emitir(
                            CategoriaNotificacao::Informativa,
                            "Histórico exportado (JSON)",
                            format!("{qtd} registro(s) salvo(s) em '{}'.", destino.display()),
                            &estado.configuracoes.notificacoes,
                        ),
                        Err(erro) => estado.notificacoes.emitir(
                            CategoriaNotificacao::ProcessamentoComFalhas,
                            "Erro na exportação JSON",
                            erro.to_string(),
                            &estado.configuracoes.notificacoes,
                        ),
                    }
                }

                if ui
                    .add_enabled(tem_registros, egui::Button::new("Exportar CSV"))
                    .clicked()
                    && let Some(destino) = rfd::FileDialog::new()
                        .set_title("Exportar histórico em CSV")
                        .set_file_name("historico_divisor_video.csv")
                        .add_filter("CSV", &["csv"])
                        .save_file()
                    && let Some(repo) = &estado.repositorio_historico
                {
                    match repo.exportar_csv(&estado.filtro_historico, &destino) {
                        Ok(qtd) => estado.notificacoes.emitir(
                            CategoriaNotificacao::Informativa,
                            "Histórico exportado (CSV)",
                            format!("{qtd} registro(s) salvo(s) em '{}'.", destino.display()),
                            &estado.configuracoes.notificacoes,
                        ),
                        Err(erro) => estado.notificacoes.emitir(
                            CategoriaNotificacao::ProcessamentoComFalhas,
                            "Erro na exportação CSV",
                            erro.to_string(),
                            &estado.configuracoes.notificacoes,
                        ),
                    }
                }
            });
        });
    });

    if recarregar {
        estado.recarregar_historico();
    }

    // Modal de confirmação explícita para limpeza total do histórico
    if estado.confirmar_limpeza_historico {
        egui::Window::new("Confirmar limpeza do histórico")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.label("Deseja realmente apagar todos os registros do histórico?");
                ui.label(
                    RichText::new(
                        "Os vídeos originais e os segmentos gerados no disco NÃO serão excluídos.",
                    )
                    .size(12.0)
                    .color(paleta.texto_secundario),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        estado.confirmar_limpeza_historico = false;
                    }
                    if ui
                        .button(
                            RichText::new("Confirmar limpeza")
                                .strong()
                                .color(paleta.erro),
                        )
                        .clicked()
                    {
                        estado.confirmar_limpeza_historico = false;
                        if let Some(repo) = &estado.repositorio_historico {
                            match repo.limpar_tudo() {
                                Ok(qtd) => {
                                    estado.notificacoes.emitir(
                                        CategoriaNotificacao::Informativa,
                                        "Histórico limpo",
                                        format!("{qtd} registro(s) removido(s) do banco local."),
                                        &estado.configuracoes.notificacoes,
                                    );
                                    estado.recarregar_historico();
                                }
                                Err(erro) => {
                                    estado.erro_historico = Some(erro.to_string());
                                }
                            }
                        }
                    }
                });
            });
    }

    ui.add_space(10.0);

    if estado.registros_historico.is_empty() {
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Nenhum registro de histórico encontrado")
                    .size(15.0)
                    .color(paleta.texto_secundario),
            );
        });
        return;
    }

    let mut acoes: Vec<AcaoHistorico> = Vec::new();

    egui::ScrollArea::vertical()
        .id_salt("scroll_historico")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for reg in &estado.registros_historico {
                let cor = match reg.resultado.as_str() {
                    "Concluído" => paleta.sucesso,
                    "Erro" => paleta.erro,
                    "Ignorado" | "Cancelado" => paleta.aviso,
                    _ => paleta.neutro,
                };
                let expandido = estado.historico_expandido.contains(&reg.id);

                frame_cartao(paleta).show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("#{}", reg.id))
                                .size(12.0)
                                .color(paleta.texto_secundario),
                        );
                        ui.label(RichText::new(&reg.nome_video).strong().size(14.0));
                        desenhar_badge_status(ui, &reg.resultado, cor);
                        ui.label(
                            RichText::new(&reg.data_hora)
                                .size(12.0)
                                .color(paleta.texto_secundario),
                        );

                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui
                                    .small_button(RichText::new("Excluir").color(paleta.erro))
                                    .clicked()
                                {
                                    acoes.push(AcaoHistorico::Excluir(reg.id));
                                }
                                if ui.small_button("Reprocessar").clicked() {
                                    acoes.push(AcaoHistorico::Reprocessar(
                                        reg.caminho_video.clone(),
                                    ));
                                }
                                if let Some(dir) = &reg.diretorio_saida
                                    && ui.small_button("Abrir pasta").clicked()
                                {
                                    acoes.push(AcaoHistorico::AbrirCaminho(dir.clone()));
                                }
                            },
                        );
                    });

                    ui.label(
                        RichText::new(reg.caminho_video.display().to_string())
                            .size(11.5)
                            .color(paleta.texto_secundario),
                    );

                    ui.label(
                        RichText::new(format!(
                            "Original: {}  •  Limite: {}  •  Partes: {}  •  Tempo: {}  •  Velocidade: {:.1} MB/s",
                            formatar_bytes_decimal(reg.tamanho_original_bytes),
                            formatar_bytes_decimal(reg.limite_configurado_bytes),
                            reg.quantidade_segmentos,
                            formatar_tempo_decorrido(Duration::from_millis(
                                reg.tempo_processamento_ms
                            )),
                            reg.velocidade_media_mb_s
                        ))
                        .size(12.0),
                    );

                    if let Some(erro) = &reg.mensagem_erro {
                        ui.label(RichText::new(erro).size(12.0).color(paleta.erro));
                    }

                    if !reg.segmentos.is_empty() {
                        ui.add_space(4.0);
                        let rotulo = if expandido {
                            format!("Ocultar segmentos ({})", reg.segmentos.len())
                        } else {
                            format!("Ver segmentos ({})", reg.segmentos.len())
                        };
                        if ui.small_button(rotulo).clicked() {
                            acoes.push(AcaoHistorico::AlternarExpandido(reg.id));
                        }

                        if expandido {
                            ui.add_space(4.0);
                            for seg in &reg.segmentos {
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
                                        acoes.push(AcaoHistorico::AbrirCaminho(
                                            seg.caminho.clone(),
                                        ));
                                    }
                                    if ui.small_button("Copiar caminho").clicked() {
                                        acoes.push(AcaoHistorico::CopiarTexto(
                                            seg.caminho.display().to_string(),
                                        ));
                                    }
                                });
                            }
                        }
                    }
                });
                ui.add_space(6.0);
            }
        });

    for acao in acoes {
        match acao {
            AcaoHistorico::AbrirCaminho(caminho) => {
                if let Err(erro) = abrir_no_sistema(&caminho) {
                    estado.notificacoes.emitir(
                        CategoriaNotificacao::Informativa,
                        "Não foi possível abrir",
                        erro.to_string(),
                        &estado.configuracoes.notificacoes,
                    );
                }
            }
            AcaoHistorico::CopiarTexto(texto) => {
                ui.ctx().copy_text(texto);
                estado.notificacoes.emitir(
                    CategoriaNotificacao::Informativa,
                    "Caminho copiado",
                    "Copiado para a área de transferência.",
                    &estado.configuracoes.notificacoes,
                );
            }
            AcaoHistorico::Reprocessar(caminho) => {
                estado.reprocessar_do_historico(caminho);
            }
            AcaoHistorico::Excluir(id) => {
                if let Some(repo) = &estado.repositorio_historico {
                    if let Err(erro) = repo.excluir_registro(id) {
                        estado.erro_historico = Some(erro.to_string());
                    } else {
                        estado.historico_expandido.remove(&id);
                        estado.recarregar_historico();
                    }
                }
            }
            AcaoHistorico::AlternarExpandido(id) => {
                if !estado.historico_expandido.remove(&id) {
                    estado.historico_expandido.insert(id);
                }
            }
        }
    }
}
