//! Testes de integração dos serviços desktop (fila assíncrona, miniaturas, histórico SQLite,
//! configurações persistentes, nomes especiais) e testes gráficos headless de todas as abas `egui`.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use divisor_video::configuracoes_persistentes::{
    ConfiguracoesPersistentes, LimitePredefinido, TemaPreferido,
};
use divisor_video::eventos::{EstadoItemFila, EventoAplicacao, InfoSegmentoGerado};
use divisor_video::executor_ffmpeg::ExecutorFfmpeg;
use divisor_video::fila_processamento::{MotorFilaProcessamento, TarefaAnalise, TarefaDivisao};
use divisor_video::historico::{FiltroHistorico, NovoRegistroHistorico, RepositorioHistorico};
use divisor_video::miniaturas::GerenciadorMiniaturas;

fn gerar_video_sintetico(destino: &Path, duracao_s: u32) {
    let saida = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc2=size=480x270:rate=25:duration={duracao_s}"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={duracao_s}"),
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v",
            "1400k",
            "-maxrate",
            "1600k",
            "-bufsize",
            "800k",
            "-g",
            "25",
            "-keyint_min",
            "25",
            "-sc_threshold",
            "0",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
        ])
        .arg(destino)
        .output()
        .expect("Falha ao executar ffmpeg nos testes desktop");

    assert!(
        saida.status.success(),
        "FFmpeg falhou: {}",
        String::from_utf8_lossy(&saida.stderr)
    );
}

#[test]
fn fila_assincrona_analisa_gera_miniatura_e_divide_sem_alterar_original() {
    let dir = TempDir::new().expect("tempdir");
    // Inclui espaços, acentos e caractere '%' no nome do arquivo.
    let video = dir.path().join("Gravação Férias (100%) #1 & 'teste'.mp4");
    let cache_dir = dir.path().join("cache_miniaturas");
    let saida_dir = dir.path().join("saida");

    gerar_video_sintetico(&video, 8);
    let bytes_orig_antes = fs::read(&video).expect("ler original antes");
    let tam_orig = bytes_orig_antes.len() as u64;
    let limite = (tam_orig / 3).max(400_000);
    let alvo = (limite * 95) / 100;

    let executor = ExecutorFfmpeg::detectar(None, None).expect("detectar ffmpeg/ffprobe");
    let diag = ExecutorFfmpeg::diagnosticar(None, None);
    assert!(diag.tudo_disponivel());
    assert!(diag.ffmpeg.versao.is_some());
    assert!(diag.ffprobe.versao.is_some());

    let motor = MotorFilaProcessamento::novo(Arc::new(|| {}));
    let miniaturas = GerenciadorMiniaturas::no_diretorio(cache_dir);

    // 1. Análise em segundo plano + geração de miniatura.
    motor.solicitar_analise(TarefaAnalise {
        id_item: 1,
        caminho: video.clone(),
        executor: executor.clone(),
        miniaturas,
    });

    let mut viu_analise_ok = false;
    let mut caminho_thumb = None;
    let prazo = Instant::now() + Duration::from_secs(20);
    while Instant::now() < prazo && !(viu_analise_ok && caminho_thumb.is_some()) {
        for ev in motor.drenar_eventos() {
            match ev {
                EventoAplicacao::AnaliseConcluida { id_item: 1, resumo } => {
                    assert_eq!(resumo.largura, 480);
                    assert_eq!(resumo.altura, 270);
                    assert_eq!(resumo.codec_video, "h264");
                    assert!(resumo.fps > 20.0);
                    viu_analise_ok = true;
                }
                EventoAplicacao::MiniaturaPronta {
                    id_item: 1,
                    caminho_imagem,
                } => {
                    caminho_thumb = Some(caminho_imagem);
                }
                EventoAplicacao::AnaliseFalhou { erro, .. } => {
                    panic!("Análise não deveria falhar: {erro}");
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    assert!(viu_analise_ok, "Análise assíncrona deve concluir");
    let thumb = caminho_thumb.expect("Miniatura deve ser gerada em cache");
    assert!(thumb.is_file());
    let rgba = GerenciadorMiniaturas::carregar_rgba(&thumb).expect("decodificar RGBA");
    assert!(rgba.largura > 0 && rgba.largura <= 240);
    assert!(rgba.altura > 0);

    // 2. Processamento assíncrono via fila.
    motor.iniciar_ou_enfileirar(
        vec![TarefaDivisao {
            id_item: 1,
            caminho: video.clone(),
            tamanho_original_bytes: tam_orig,
            diretorio_saida: saida_dir,
            tamanho_maximo_bytes: limite,
            tamanho_alvo_bytes: alvo,
            permitir_sobrescrita: false,
            executor,
        }],
        2,
    );

    let mut resultado_item = None;
    let mut resumo_fila = None;
    let prazo_div = Instant::now() + Duration::from_secs(30);
    while Instant::now() < prazo_div && resumo_fila.is_none() {
        for ev in motor.drenar_eventos() {
            match ev {
                EventoAplicacao::ItemFinalizado {
                    id_item: 1,
                    resultado,
                } => {
                    resultado_item = Some(resultado);
                }
                EventoAplicacao::FilaFinalizada { resumo, cancelada } => {
                    assert!(!cancelada);
                    resumo_fila = Some(resumo);
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let item_final = resultado_item.expect("Item deve finalizar");
    assert_eq!(item_final.estado, EstadoItemFila::Concluido);
    assert!(item_final.segmentos.len() >= 2);
    for seg in &item_final.segmentos {
        assert!(seg.tamanho_bytes <= limite);
        assert!(seg.caminho.is_file());
    }

    let fila_final = resumo_fila.expect("Fila deve finalizar com resumo");
    assert_eq!(fila_final.videos_divididos, 1);
    assert_eq!(fila_final.falhas, 0);
    assert_eq!(fila_final.segmentos_gerados, item_final.segmentos.len());

    // Original permanece byte-a-byte intacto.
    let bytes_orig_depois = fs::read(&video).expect("ler original depois");
    assert_eq!(bytes_orig_antes, bytes_orig_depois);
}

#[test]
fn exclusao_de_historico_nao_apaga_video_original_nem_segmentos() {
    let dir = TempDir::new().expect("tempdir");
    let original = dir.path().join("orig.mp4");
    let segmento = dir.path().join("orig_parte_001.mp4");
    fs::write(&original, b"video_original_preservado").expect("write orig");
    fs::write(&segmento, b"segmento_preservado").expect("write seg");

    let repo = RepositorioHistorico::abrir(&dir.path().join("hist.sqlite3")).expect("abrir db");
    let id = repo
        .inserir(&NovoRegistroHistorico {
            nome_video: "orig.mp4".to_string(),
            caminho_video: original.clone(),
            data_hora: None,
            tamanho_original_bytes: 12_000_000,
            limite_configurado_bytes: 10_000_000,
            segmentos: vec![InfoSegmentoGerado {
                indice: 1,
                nome_arquivo: "orig_parte_001.mp4".to_string(),
                caminho: segmento.clone(),
                tamanho_bytes: 9_500_000,
                duracao_segundos: 5.0,
            }],
            diretorio_saida: Some(dir.path().to_path_buf()),
            tempo_processamento_ms: 400,
            velocidade_media_mb_s: 30.0,
            resultado: "Concluído".to_string(),
            mensagem_erro: None,
        })
        .expect("inserir");

    repo.excluir_registro(id).expect("excluir");
    assert!(
        repo.consultar(&FiltroHistorico::default())
            .expect("consultar")
            .is_empty()
    );
    assert!(original.is_file(), "Vídeo original nunca deve ser excluído");
    assert!(
        segmento.is_file(),
        "Segmento gerado nunca deve ser excluído"
    );
}

#[cfg(feature = "gui")]
#[test]
fn renderizacao_headless_de_todas_as_abas_e_interacoes_de_estado() {
    use divisor_video::gui::aba_configuracoes::renderizar_aba_configuracoes;
    use divisor_video::gui::aba_historico::renderizar_aba_historico;
    use divisor_video::gui::aba_inicio::renderizar_aba_inicio;
    use divisor_video::gui::aba_sobre::renderizar_aba_sobre;
    use divisor_video::gui::componentes::{PaletaCores, desenhar_toasts};
    use divisor_video::gui::estado::{AbaAtiva, EstadoAplicacao};

    let dir = TempDir::new().expect("tempdir");
    let v1 = dir.path().join("a.mp4");
    let v2 = dir.path().join("b.mkv");
    let invalido = dir.path().join("documento.pdf");
    fs::write(&v1, b"fake1").expect("v1");
    fs::write(&v2, b"fake2").expect("v2");
    fs::write(&invalido, b"pdf").expect("pdf");

    let mut estado = EstadoAplicacao::novo(Arc::new(|| {}));
    estado.configuracoes = ConfiguracoesPersistentes {
        diretorio_saida: dir.path().join("saida_gui"),
        ..Default::default()
    };
    estado
        .configuracoes
        .notificacoes
        .habilitar_notificacoes_sistema = false;

    // 1. Testa detecção de duplicados e rejeição de formatos inválidos.
    estado.adicionar_caminhos(vec![v1.clone(), v2.clone(), v1.clone(), invalido], false);
    assert_eq!(estado.itens_fila.len(), 2);
    assert_eq!(estado.itens_fila[0].nome_arquivo, "a.mp4");
    assert_eq!(estado.itens_fila[1].nome_arquivo, "b.mkv");

    // 2. Testa reordenação na fila.
    let id_b = estado.itens_fila[1].id;
    estado.itens_fila[0].estado = EstadoItemFila::AguardandoProcessamento;
    estado.itens_fila[1].estado = EstadoItemFila::AguardandoProcessamento;
    estado.mover_item_para_cima(id_b);
    assert_eq!(estado.itens_fila[0].nome_arquivo, "b.mkv");
    estado.mover_item_para_baixo(id_b);
    assert_eq!(estado.itens_fila[0].nome_arquivo, "a.mp4");

    // 3. Testa seleção de limite personalizado e margem automática.
    estado.campo_limite_personalizado_mb = "25".to_string();
    estado.aplicar_limite_personalizado();
    assert_eq!(
        estado.configuracoes.limite_predefinido,
        LimitePredefinido::Personalizado
    );
    assert_eq!(
        estado.configuracoes.limite_maximo_efetivo_bytes(),
        25_000_000
    );
    assert!(estado.configuracoes.tamanho_alvo_efetivo_bytes() < 25_000_000);

    // 4. Renderiza todas as 4 abas em modo escuro e modo claro sem GPU usando egui::Context::run_ui.
    let ctx = egui::Context::default();
    for tema in [TemaPreferido::Escuro, TemaPreferido::Claro] {
        estado.configuracoes.tema = tema;
        let escuro = tema == TemaPreferido::Escuro;
        let paleta = PaletaCores::para_tema(escuro);

        for aba in [
            AbaAtiva::Inicio,
            AbaAtiva::Historico,
            AbaAtiva::Configuracoes,
            AbaAtiva::SobreAjuda,
        ] {
            estado.aba_ativa = aba;
            let raw_input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(1200.0, 800.0),
                )),
                ..Default::default()
            };

            let mut saida = ctx.run_ui(raw_input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| match estado.aba_ativa {
                    AbaAtiva::Inicio => renderizar_aba_inicio(ui, &mut estado, paleta),
                    AbaAtiva::Historico => renderizar_aba_historico(ui, &mut estado, paleta),
                    AbaAtiva::Configuracoes => {
                        renderizar_aba_configuracoes(ui, &mut estado, paleta);
                    }
                    AbaAtiva::SobreAjuda => renderizar_aba_sobre(ui, &estado, paleta),
                });
                let _ = desenhar_toasts(ui.ctx(), estado.notificacoes.ativas(), paleta);
            });

            assert!(
                !saida.shapes.is_empty(),
                "Aba {aba:?} no tema {tema:?} deve produzir primitivas de renderização"
            );
            // Em renderização headless sem GPU, descartamos explicitamente os deltas de textura.
            saida.textures_delta.clear();
        }
    }

    // 5. Remoção individual e limpeza da fila.
    let id_a = estado.itens_fila[0].id;
    estado.remover_item(id_a);
    assert_eq!(estado.itens_fila.len(), 1);
    estado.limpar_fila();
    assert!(estado.itens_fila.is_empty());
}
