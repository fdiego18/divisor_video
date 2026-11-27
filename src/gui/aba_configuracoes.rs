//! Aba Configurações: tema, idioma, limites de tamanho, diretório de saída,
//! concorrência, notificações, diagnóstico/seleção de FFmpeg/FFprobe e cache local.

use egui::{ComboBox, RichText, Ui};

use crate::configuracoes_persistentes::{
    ConfiguracoesPersistentes, LimitePredefinido, TemaPreferido,
};
use crate::gui::componentes::{
    PaletaCores, desenhar_badge_status, desenhar_cabecalho_secao, formatar_bytes_decimal,
    frame_cartao,
};
use crate::gui::estado::EstadoAplicacao;
use crate::integracao_so::diretorio_saida_padrao;
use crate::notificacoes::CategoriaNotificacao;

/// Renderiza todo o conteúdo da aba **Configurações**.
pub fn renderizar_aba_configuracoes(
    ui: &mut Ui,
    estado: &mut EstadoAplicacao,
    paleta: PaletaCores,
) {
    desenhar_cabecalho_secao(
        ui,
        "Configurações",
        Some(
            "Todas as alterações são salvas automaticamente no diretório de configurações do sistema.",
        ),
        paleta,
    );

    let mut salvar = false;
    let mut rediagnosticar = false;

    egui::ScrollArea::vertical()
        .id_salt("scroll_configuracoes")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // 1. Aparência e Idioma
            frame_cartao(paleta).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Aparência e Idioma").size(15.0).strong());
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    ui.label("Tema visual:");
                    let mut tema = estado.configuracoes.tema;
                    ComboBox::from_id_salt("combo_tema_cfg")
                        .selected_text(tema.rotulo())
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for &opcao in TemaPreferido::TODAS {
                                ui.selectable_value(&mut tema, opcao, opcao.rotulo());
                            }
                        });
                    if tema != estado.configuracoes.tema {
                        estado.configuracoes.tema = tema;
                        salvar = true;
                    }

                    ui.add_space(16.0);
                    ui.label("Idioma:");
                    ui.label(RichText::new("Português (Brasil) — pt-BR").strong());
                });
            });

            ui.add_space(10.0);

            // 2. Limite de tamanho e Saída
            frame_cartao(paleta).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Processamento e Diretório de Saída").size(15.0).strong());
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    ui.label("Limite padrão por segmento:");
                    let mut lim = estado.configuracoes.limite_predefinido;
                    ComboBox::from_id_salt("combo_limite_cfg")
                        .selected_text(lim.rotulo())
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for &opcao in LimitePredefinido::TODAS {
                                ui.selectable_value(&mut lim, opcao, opcao.rotulo());
                            }
                        });
                    if lim != estado.configuracoes.limite_predefinido {
                        estado.configuracoes.limite_predefinido = lim;
                        salvar = true;
                    }

                    if estado.configuracoes.limite_predefinido == LimitePredefinido::Personalizado {
                        ui.label("Valor personalizado (MB):");
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut estado.campo_limite_personalizado_mb)
                                .desired_width(80.0),
                        );
                        if resp.lost_focus() || ui.button("Aplicar").clicked() {
                            estado.aplicar_limite_personalizado();
                        }
                    }
                });

                ui.label(
                    RichText::new(format!(
                        "Unidade decimal (1 MB = 1.000.000 bytes) • Limite: {} • Alvo automático: {} • Margem: {}",
                        formatar_bytes_decimal(estado.configuracoes.limite_maximo_efetivo_bytes()),
                        formatar_bytes_decimal(estado.configuracoes.tamanho_alvo_efetivo_bytes()),
                        formatar_bytes_decimal(estado.configuracoes.margem_seguranca_bytes())
                    ))
                    .size(12.0)
                    .color(paleta.texto_secundario),
                );

                ui.add_space(8.0);

                ui.horizontal_wrapped(|ui| {
                    ui.label("Diretório padrão de saída:");
                    ui.label(
                        RichText::new(estado.configuracoes.diretorio_saida.display().to_string())
                            .monospace()
                            .size(12.0),
                    );
                    if ui.small_button("Selecionar...").clicked()
                        && let Some(pasta) = rfd::FileDialog::new()
                            .set_title("Selecionar diretório padrão de saída")
                            .pick_folder()
                    {
                        estado.configuracoes.diretorio_saida = pasta;
                        salvar = true;
                    }
                    if ui.small_button("Usar padrão").clicked() {
                        estado.configuracoes.diretorio_saida = diretorio_saida_padrao();
                        salvar = true;
                    }
                });

                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    ui.label("Processamentos simultâneos:");
                    let mut conc = estado.configuracoes.concorrencia;
                    if ui
                        .add(egui::Slider::new(&mut conc, 1..=16).text("worker(s)"))
                        .changed()
                    {
                        estado.configuracoes.concorrencia = conc;
                        salvar = true;
                    }
                });

                if ui
                    .checkbox(
                        &mut estado.configuracoes.abrir_pasta_ao_concluir,
                        "Abrir automaticamente o diretório de saída ao concluir a fila",
                    )
                    .changed()
                {
                    salvar = true;
                }
                if ui
                    .checkbox(
                        &mut estado.configuracoes.busca_recursiva,
                        "Ativar busca recursiva em subdiretórios ao adicionar pastas",
                    )
                    .changed()
                {
                    salvar = true;
                }
                if ui
                    .checkbox(
                        &mut estado.configuracoes.permitir_sobrescrita,
                        "Permitir sobrescrita de segmentos existentes no diretório de saída",
                    )
                    .changed()
                {
                    salvar = true;
                }
            });

            ui.add_space(10.0);

            // 3. FFmpeg e FFprobe
            frame_cartao(paleta).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("FFmpeg e FFprobe").size(15.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Revalidar agora").clicked() {
                            rediagnosticar = true;
                        }
                        if ui.small_button("Detectar automaticamente").clicked() {
                            estado.configuracoes.caminho_ffmpeg = None;
                            estado.configuracoes.caminho_ffprobe = None;
                            salvar = true;
                            rediagnosticar = true;
                        }
                    });
                });
                ui.label(
                    RichText::new(
                        "Ordem de detecção: 1) Caminho personalizado → 2) Diretório do aplicativo → 3) Pasta de ferramentas → 4) PATH do sistema.",
                    )
                    .size(12.0)
                    .color(paleta.texto_secundario),
                );
                ui.add_space(8.0);

                // Diagnóstico FFmpeg
                renderizar_linha_ferramenta(
                    ui,
                    "FFmpeg",
                    &estado.diagnostico.ffmpeg,
                    paleta,
                    || {
                        rfd::FileDialog::new()
                            .set_title("Selecionar executável FFmpeg")
                            .pick_file()
                    },
                    |novo| {
                        estado.configuracoes.caminho_ffmpeg = Some(novo);
                        salvar = true;
                        rediagnosticar = true;
                    },
                );

                ui.add_space(6.0);

                // Diagnóstico FFprobe
                renderizar_linha_ferramenta(
                    ui,
                    "FFprobe",
                    &estado.diagnostico.ffprobe,
                    paleta,
                    || {
                        rfd::FileDialog::new()
                            .set_title("Selecionar executável FFprobe")
                            .pick_file()
                    },
                    |novo| {
                        estado.configuracoes.caminho_ffprobe = Some(novo);
                        salvar = true;
                        rediagnosticar = true;
                    },
                );
            });

            ui.add_space(10.0);

            // 4. Notificações
            frame_cartao(paleta).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Notificações").size(15.0).strong());
                ui.add_space(6.0);

                let n = &mut estado.configuracoes.notificacoes;
                if ui
                    .checkbox(
                        &mut n.habilitar_notificacoes_sistema,
                        "Enviar notificações nativas do sistema operacional",
                    )
                    .changed()
                {
                    salvar = true;
                }
                ui.separator();
                if ui
                    .checkbox(&mut n.processamento_concluido, "Processamento individual concluído")
                    .changed()
                {
                    salvar = true;
                }
                if ui
                    .checkbox(&mut n.processamento_com_falhas, "Processamento com falhas")
                    .changed()
                {
                    salvar = true;
                }
                if ui
                    .checkbox(&mut n.fila_finalizada, "Fila finalizada (com resumo)")
                    .changed()
                {
                    salvar = true;
                }
                if ui
                    .checkbox(&mut n.falta_espaco_disco, "Falta de espaço em disco")
                    .changed()
                {
                    salvar = true;
                }
                if ui
                    .checkbox(&mut n.cancelamento, "Cancelamento de processamento")
                    .changed()
                {
                    salvar = true;
                }
                if ui
                    .checkbox(&mut n.dependencias_indisponiveis, "Dependências indisponíveis")
                    .changed()
                {
                    salvar = true;
                }
            });

            ui.add_space(10.0);

            // 5. Cache e Caminhos Locais
            frame_cartao(paleta).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Armazenamento Local e Cache").size(15.0).strong());
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Configurações: {}",
                        ConfiguracoesPersistentes::caminho_padrao().display()
                    ))
                    .size(12.0)
                    .color(paleta.texto_secundario),
                );
                ui.label(
                    RichText::new(format!(
                        "Cache de miniaturas: {}",
                        estado.miniaturas.diretorio_cache().display()
                    ))
                    .size(12.0)
                    .color(paleta.texto_secundario),
                );
                ui.add_space(6.0);
                if ui.button("Limpar cache de miniaturas").clicked() {
                    match estado.miniaturas.limpar_cache() {
                        Ok(qtd) => {
                            estado.texturas_miniaturas.clear();
                            estado.ordem_texturas.clear();
                            estado.notificacoes.emitir(
                                CategoriaNotificacao::Informativa,
                                "Cache limpo",
                                format!("{qtd} miniatura(s) removida(s) do cache local."),
                                &estado.configuracoes.notificacoes,
                            );
                        }
                        Err(erro) => {
                            estado.notificacoes.emitir(
                                CategoriaNotificacao::ProcessamentoComFalhas,
                                "Falha ao limpar cache",
                                erro.to_string(),
                                &estado.configuracoes.notificacoes,
                            );
                        }
                    }
                }
            });
        });

    if salvar {
        estado.salvar_configuracoes();
    }
    if rediagnosticar {
        estado.atualizar_diagnostico_ferramentas();
    }
}

/// Desenha o bloco de diagnóstico + seletor manual de um executável multimídia.
fn renderizar_linha_ferramenta<FSel, FApl>(
    ui: &mut Ui,
    rotulo: &str,
    info: &crate::executor_ffmpeg::InfoFerramenta,
    paleta: PaletaCores,
    selecionar: FSel,
    mut ao_aplicar: FApl,
) where
    FSel: FnOnce() -> Option<std::path::PathBuf>,
    FApl: FnMut(std::path::PathBuf),
{
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{rotulo}:")).strong());
        if info.disponivel {
            desenhar_badge_status(ui, "Disponível", paleta.sucesso);
        } else {
            desenhar_badge_status(ui, "Indisponível", paleta.erro);
        }

        if let Some(origem) = info.origem {
            ui.label(
                RichText::new(format!("({})", origem.descricao()))
                    .size(12.0)
                    .color(paleta.texto_secundario),
            );
        }

        if ui.small_button("Selecionar executável...").clicked()
            && let Some(arq) = selecionar()
        {
            ao_aplicar(arq);
        }
    });

    if let Some(caminho) = &info.caminho {
        ui.label(
            RichText::new(format!("  Caminho: {}", caminho.display()))
                .monospace()
                .size(11.5),
        );
    }
    if let Some(versao) = &info.versao {
        ui.label(
            RichText::new(format!("  Versão: {versao}"))
                .size(11.5)
                .color(paleta.texto_secundario),
        );
    }
    if let Some(erro) = &info.erro {
        ui.label(
            RichText::new(format!("  Diagnóstico: {erro}"))
                .size(11.5)
                .color(paleta.erro),
        );
    }
}
