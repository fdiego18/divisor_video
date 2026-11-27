//! Aba Sobre e Ajuda: informações da versão, garantias de preservação de qualidade,
//! guia de utilização, privacidade offline e notas de licenciamento.

use egui::{RichText, Ui};

use crate::gui::componentes::{PaletaCores, desenhar_cabecalho_secao, frame_cartao};
use crate::gui::estado::EstadoAplicacao;

/// Renderiza todo o conteúdo da aba **Sobre e Ajuda**.
pub fn renderizar_aba_sobre(ui: &mut Ui, estado: &EstadoAplicacao, paleta: PaletaCores) {
    desenhar_cabecalho_secao(
        ui,
        "Sobre e Ajuda",
        Some("Divisor de Vídeo — Aplicativo Desktop e CLI em Rust Edition 2024"),
        paleta,
    );

    egui::ScrollArea::vertical()
        .id_salt("scroll_sobre")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            frame_cartao(paleta).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!("Divisor de Vídeo v{}", env!("CARGO_PKG_VERSION")))
                        .size(16.0)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(
                    "Ferramenta multiplataforma desenvolvida em Rust para dividir vídeos em múltiplos segmentos \
                     respeitando limites rígidos de tamanho (como 10 MB para plataformas de upload), preservando \
                     100% da qualidade audiovisual original por meio de Stream Copy (-c copy).",
                );
            });

            ui.add_space(10.0);

            frame_cartao(paleta).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Garantias Técnicas e Privacidade").size(15.0).strong());
                ui.add_space(4.0);
                for ponto in [
                    "• Zero recodificação: mantém resolução, FPS, bitrate, codecs de vídeo e todas as faixas de áudio originais.",
                    "• Cortes exclusivamente em keyframes: garante reprodução independente de cada segmento, sem perda nem duplicação de quadros.",
                    "• Validação rigorosa pós-geração: nenhum segmento acima do limite configurado é publicado; caso necessário, um keyframe anterior é selecionado automaticamente.",
                    "• Se um único intervalo entre keyframes (GOP) exceder o limite configurado, o aplicativo informa claramente o motivo sem reduzir a qualidade.",
                    "• Arquivos originais nunca são modificados ou removidos.",
                    "• 100% offline: nenhum dado é enviado para serviços externos e não existe qualquer telemetria.",
                ] {
                    ui.label(RichText::new(ponto).size(12.5));
                }
            });

            ui.add_space(10.0);

            frame_cartao(paleta).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Como Usar Rapidamente").size(15.0).strong());
                ui.add_space(4.0);
                for passo in [
                    "1. Na aba Início, arraste e solte arquivos ou pastas de vídeo (ou use os botões de seleção).",
                    "2. Escolha o limite por segmento (10 MB padrão ou qualquer opção de 5 MB até 1 GB / personalizado).",
                    "3. Selecione o diretório de saída desejado e clique em \"Iniciar processamento\".",
                    "4. Clique em \"Ver segmentos gerados\" para abrir qualquer parte no reprodutor padrão ou copiar o caminho.",
                    "5. Consulte processamentos anteriores, filtre ou exporte relatórios em CSV/JSON na aba Histórico.",
                ] {
                    ui.label(RichText::new(passo).size(12.5));
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Estrutura padrão de saída:")
                        .size(12.5)
                        .strong(),
                );
                ui.label(
                    RichText::new(
                        "  saida/\n  └── meu_video/\n      ├── meu_video_parte_001.mp4\n      ├── meu_video_parte_002.mp4\n      └── meu_video_parte_003.mp4",
                    )
                    .monospace()
                    .size(12.0)
                    .color(paleta.texto_secundario),
                );
            });

            ui.add_space(10.0);

            frame_cartao(paleta).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Ambiente Atual e Licenças").size(15.0).strong());
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Sistema: {} ({}) • FFmpeg: {} • FFprobe: {}",
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                        if estado.diagnostico.ffmpeg.disponivel {
                            "Disponível"
                        } else {
                            "Indisponível"
                        },
                        if estado.diagnostico.ffprobe.disponivel {
                            "Disponível"
                        } else {
                            "Indisponível"
                        },
                    ))
                    .size(12.5),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Licença do aplicativo: MIT. O processamento multimídia utiliza FFmpeg e FFprobe, \
                         distribuídos sob suas respectivas licenças (LGPL v2.1+ ou GPL v2+/v3+ conforme a compilação \
                         incluída). Consulte docs/DISTRIBUICAO_E_LICENCAS.md para obrigações de redistribuição.",
                    )
                    .size(12.0)
                    .color(paleta.texto_secundario),
                );
            });
        });
}
