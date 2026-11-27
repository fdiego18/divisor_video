//! Testes de integração ponta a ponta utilizando FFmpeg e FFprobe reais.
//!
//! Cada teste gera vídeos sintéticos controlados em diretórios temporários isolados
//! (limpos automaticamente ao término) e valida todas as garantias de tamanho,
//! preservação de codecs, integridade de pacotes e continuidade temporal.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tempfile::TempDir;

use divisor_video::analisador_video::{analisar_metadados, analisar_video};
use divisor_video::configuracao::{
    Configuracao, TAMANHO_ALVO_PADRAO_BYTES, TAMANHO_MAXIMO_PADRAO_BYTES,
};
use divisor_video::divisor_video::{StatusProcessamentoVideo, dividir_arquivo_video};
use divisor_video::erros::ErroDivisor;
use divisor_video::executor_ffmpeg::ExecutorFfmpeg;
use divisor_video::processamento_lote::{ResultadoItemLote, executar_lote};
use divisor_video::progresso::GerenciadorProgresso;

/// Helper para gerar vídeos sintéticos via FFmpeg falhando com diagnóstico claro se algo der errado.
fn gerar_video_ffmpeg(args: &[&str], destino: &Path) {
    let saida = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-y")
        .arg("-v")
        .arg("error")
        .args(args)
        .arg(destino)
        .output()
        .expect("Falha ao invocar ffmpeg nos testes de integração");

    assert!(
        saida.status.success(),
        "FFmpeg falhou ao gerar vídeo de teste '{}': {}",
        destino.display(),
        String::from_utf8_lossy(&saida.stderr)
    );
}

fn criar_executor() -> ExecutorFfmpeg {
    ExecutorFfmpeg::detectar(None, None).expect("FFmpeg e FFprobe devem estar disponíveis no PATH")
}

/// Teste CBR com o limite padrão real de 10.000.000 bytes (10 MB) e alvo de 9.800.000 bytes.
///
/// Também valida nomenclatura exata, preservação do original, codecs, integridade e continuidade.
#[test]
fn divide_video_cbr_respeitando_limite_padrao_10mb_e_preservando_qualidade() {
    let dir = TempDir::new().expect("Falha ao criar tempdir");
    let video = dir.path().join("video_cbr.mp4");
    let saida = dir.path().join("saida");

    // Gera vídeo CBR > 15 MB (12s a ~12 Mbps com ruído controlado, GOP fixo de 1s = 25 frames).
    gerar_video_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1280x720:rate=25:duration=12,noise=alls=25:allf=t",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=12",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v",
            "12000k",
            "-minrate",
            "12000k",
            "-maxrate",
            "12000k",
            "-bufsize",
            "2400k",
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
        ],
        &video,
    );

    let tamanho_original = fs::metadata(&video).expect("metadata").len();
    assert!(
        tamanho_original > TAMANHO_MAXIMO_PADRAO_BYTES,
        "Vídeo de teste CBR deveria exceder 10 MB (gerado: {tamanho_original} bytes)"
    );

    let executor = criar_executor();
    let config = Configuracao::nova(video.clone(), saida.clone(), TAMANHO_MAXIMO_PADRAO_BYTES)
        .expect("config");
    assert_eq!(config.tamanho_maximo_bytes, 10_000_000);
    assert_eq!(config.tamanho_alvo_bytes, 9_800_000);

    let cancelamento = Arc::new(AtomicBool::new(false));
    let progresso = GerenciadorProgresso::novo(1, true);
    let mapa_orig = analisar_video(&executor, &video, &cancelamento).expect("analisar original");

    let relatorio = dividir_arquivo_video(&video, &config, &executor, &progresso, &cancelamento)
        .expect("divisão CBR deve ter sucesso");

    // Arquivo original deve permanecer intacto.
    assert!(video.exists());
    assert_eq!(
        fs::metadata(&video).expect("meta orig").len(),
        tamanho_original
    );

    match relatorio.status {
        StatusProcessamentoVideo::Dividido {
            diretorio_destino,
            segmentos,
            validacao,
            ..
        } => {
            assert_eq!(diretorio_destino, saida.join("video_cbr"));
            assert!(segmentos.len() >= 2, "Deve gerar ao menos 2 partes");

            // Nomenclatura obrigatória e validação do limite absoluto de 10.000.000 bytes.
            for (i, seg) in segmentos.iter().enumerate() {
                let esperado = diretorio_destino.join(format!("video_cbr_parte_{:03}.mp4", i + 1));
                assert_eq!(seg.caminho, esperado);
                assert!(seg.caminho.is_file());
                let tam_real = fs::metadata(&seg.caminho).expect("meta seg").len();
                assert!(
                    tam_real <= TAMANHO_MAXIMO_PADRAO_BYTES,
                    "Segmento {} excedeu 10.000.000 bytes: {tam_real}",
                    seg.indice
                );
                assert!(seg.inicia_em_keyframe, "Segmento deve iniciar em keyframe");

                // Verifica preservação de parâmetros audiovisuais via FFprobe.
                let meta_seg = analisar_metadados(&executor, &seg.caminho).expect("meta seg");
                let v_orig = mapa_orig
                    .metadados
                    .stream_video_principal()
                    .expect("v_orig");
                let v_seg = meta_seg.stream_video_principal().expect("v_seg");
                assert_eq!(v_orig.codec_nome, v_seg.codec_nome);
                assert_eq!(v_orig.largura, v_seg.largura);
                assert_eq!(v_orig.altura, v_seg.altura);
                assert_eq!(v_orig.formato_pixel, v_seg.formato_pixel);
            }

            // Integridade exata de pacotes de vídeo e continuidade temporal.
            assert_eq!(
                validacao.total_pacotes_video_segmentos,
                mapa_orig.total_pacotes_video
            );
            assert!(
                (validacao.duracao_total_segmentos - mapa_orig.metadados.duracao_segundos).abs()
                    < 0.5
            );
            // Aproveitamento razoável no primeiro segmento (não gerar partes minúsculas).
            assert!(
                segmentos[0].tamanho_bytes > 6_000_000,
                "Primeiro segmento deveria aproveitar bem o limite de 10 MB (gerado: {} bytes)",
                segmentos[0].tamanho_bytes
            );
        }
        StatusProcessamentoVideo::IgnoradoAbaixoDoLimite { .. } => {
            panic!("Vídeo > 10 MB não deveria ter sido ignorado");
        }
    }
}

/// 2. Teste com vídeo VBR (forte variação de bitrate entre cenas estáticas e cenas de alto movimento).
#[test]
fn divide_video_vbr_dinamicamente_sem_estourar_limite() {
    let dir = TempDir::new().expect("tempdir");
    let video = dir.path().join("video_vbr.mp4");
    let saida = dir.path().join("saida");

    // Alterna cena simples (baixo bitrate) com cena de ruído aleatório (altíssimo bitrate em CRF fixo).
    gerar_video_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=640x360:r=25:d=4",
            "-f",
            "lavfi",
            "-i",
            "nullsrc=s=640x360:r=25:d=4,noise=alls=80:allf=t",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=640x360:r=25:d=4",
            "-f",
            "lavfi",
            "-i",
            "nullsrc=s=640x360:r=25:d=4,noise=alls=80:allf=t",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=520:duration=16",
            "-filter_complex",
            "[0:v][1:v][2:v][3:v]concat=n=4:v=1:a=0[v]",
            "-map",
            "[v]",
            "-map",
            "4:a",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "24",
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
        ],
        &video,
    );

    let tamanho_original = fs::metadata(&video).expect("meta").len();
    // Define limite ~40% do arquivo para forçar múltiplos cortes em regime altamente VBR.
    let limite = (tamanho_original / 3).max(500_000);

    let executor = criar_executor();
    let config = Configuracao::nova(video.clone(), saida, limite).expect("config");
    let cancelamento = Arc::new(AtomicBool::new(false));
    let progresso = GerenciadorProgresso::novo(1, true);

    let relatorio = dividir_arquivo_video(&video, &config, &executor, &progresso, &cancelamento)
        .expect("divisão VBR deve ter sucesso");

    match relatorio.status {
        StatusProcessamentoVideo::Dividido {
            segmentos, eh_vbr, ..
        } => {
            assert!(eh_vbr, "Analisador deveria classificar o vídeo como VBR");
            assert!(segmentos.len() >= 3);
            for seg in &segmentos {
                assert!(
                    seg.tamanho_bytes <= limite,
                    "Segmento {} excedeu o limite VBR ({} > {})",
                    seg.indice,
                    seg.tamanho_bytes,
                    limite
                );
                assert!(seg.inicia_em_keyframe);
            }
        }
        StatusProcessamentoVideo::IgnoradoAbaixoDoLimite { .. } => {
            panic!("Vídeo VBR não deveria ser ignorado");
        }
    }
}

/// 3. Teste com vídeo contendo múltiplas faixas de áudio (3 faixas distintas).
#[test]
fn preserva_multiplas_faixas_de_audio_integralmente() {
    let dir = TempDir::new().expect("tempdir");
    let video = dir.path().join("multi_audio.mkv");
    let saida = dir.path().join("saida");

    gerar_video_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=480x270:rate=25:duration=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=44100:duration=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1320:sample_rate=48000:duration=10",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-map",
            "3:a",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v",
            "1500k",
            "-g",
            "25",
            "-keyint_min",
            "25",
            "-sc_threshold",
            "0",
            "-c:a:0",
            "aac",
            "-b:a:0",
            "128k",
            "-c:a:1",
            "libmp3lame",
            "-b:a:1",
            "128k",
            "-c:a:2",
            "aac",
            "-b:a:2",
            "96k",
        ],
        &video,
    );

    let tamanho_orig = fs::metadata(&video).expect("meta").len();
    let limite = (tamanho_orig / 3).max(400_000);

    let executor = criar_executor();
    let config = Configuracao::nova(video.clone(), saida, limite).expect("config");
    let cancelamento = Arc::new(AtomicBool::new(false));
    let progresso = GerenciadorProgresso::novo(1, true);

    let relatorio = dividir_arquivo_video(&video, &config, &executor, &progresso, &cancelamento)
        .expect("divisão multi-áudio deve ter sucesso");

    match relatorio.status {
        StatusProcessamentoVideo::Dividido { segmentos, .. } => {
            assert!(segmentos.len() >= 2);
            for seg in &segmentos {
                assert!(seg.tamanho_bytes <= limite);
                let meta = analisar_metadados(&executor, &seg.caminho).expect("meta seg");
                assert_eq!(
                    meta.streams_audio.len(),
                    3,
                    "Todas as 3 faixas de áudio devem ser preservadas em cada segmento"
                );
                assert_eq!(meta.streams_audio[0].codec_nome, "aac");
                assert_eq!(meta.streams_audio[1].codec_nome, "mp3");
                assert_eq!(meta.streams_audio[2].codec_nome, "aac");
            }
        }
        StatusProcessamentoVideo::IgnoradoAbaixoDoLimite { .. } => {
            panic!("Vídeo multi-áudio não deveria ser ignorado");
        }
    }
}

/// 4. Teste com intervalos longos entre keyframes:
/// - Quando 1 único GOP excede o limite, deve recusar recodificação e informar claramente o motivo.
/// - Nenhum arquivo parcial/inválido deve restar na pasta de saída.
#[test]
fn recusa_divisao_quando_intervalo_entre_keyframes_excede_limite() {
    let dir = TempDir::new().expect("tempdir");
    let video = dir.path().join("gop_longo.mp4");
    let saida = dir.path().join("saida");

    // 1 único keyframe no início para 8 segundos inteiros (~2 MB em um único GOP).
    gerar_video_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=25:duration=8",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=8",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v",
            "2000k",
            "-g",
            "500",
            "-keyint_min",
            "500",
            "-sc_threshold",
            "0",
            "-c:a",
            "aac",
        ],
        &video,
    );

    let tamanho_orig = fs::metadata(&video).expect("meta").len();
    // Limite menor que o único GOP (metade do arquivo).
    let limite = tamanho_orig / 2;

    let executor = criar_executor();
    let config = Configuracao::nova(video.clone(), saida.clone(), limite).expect("config");
    let cancelamento = Arc::new(AtomicBool::new(false));
    let progresso = GerenciadorProgresso::novo(1, true);

    let resultado = dividir_arquivo_video(&video, &config, &executor, &progresso, &cancelamento);
    assert!(
        matches!(
            resultado,
            Err(ErroDivisor::DivisaoImpossivelSemRecodificacao { .. })
        ),
        "Esperado DivisaoImpossivelSemRecodificacao, obtido: {resultado:?}"
    );

    // Nenhum arquivo inválido ou temporário deve ficar no destino final.
    let pasta_video = saida.join("gop_longo");
    assert!(
        !pasta_video.exists(),
        "Diretório final não deveria conter segmentos inválidos"
    );
}

/// 5. Teste com arquivo corrompido: retorna `ArquivoCorrompido` com segurança.
#[test]
fn rejeita_arquivo_corrompido_com_erro_claro() {
    let dir = TempDir::new().expect("tempdir");
    let corrompido = dir.path().join("corrompido.mp4");
    let saida = dir.path().join("saida");

    fs::write(
        &corrompido,
        b"isto nao e um arquivo mp4 valido 0000111122223333",
    )
    .expect("gravar arquivo corrompido");

    let executor = criar_executor();
    let config =
        Configuracao::nova(corrompido.clone(), saida, TAMANHO_MAXIMO_PADRAO_BYTES).expect("config");
    let cancelamento = Arc::new(AtomicBool::new(false));
    let progresso = GerenciadorProgresso::novo(1, true);

    let resultado =
        dividir_arquivo_video(&corrompido, &config, &executor, &progresso, &cancelamento);
    assert!(
        matches!(resultado, Err(ErroDivisor::ArquivoCorrompido { .. })),
        "Esperado ArquivoCorrompido, obtido: {resultado:?}"
    );
}

/// 6. Teste com arquivo menor que 10 MB: deve ser ignorado sem criar cópias desnecessárias.
#[test]
fn ignora_video_menor_que_10mb() {
    let dir = TempDir::new().expect("tempdir");
    let pequeno = dir.path().join("pequeno.mp4");
    let saida = dir.path().join("saida");

    gerar_video_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v",
            "400k",
            "-c:a",
            "aac",
        ],
        &pequeno,
    );

    assert!(fs::metadata(&pequeno).expect("meta").len() < TAMANHO_MAXIMO_PADRAO_BYTES);

    let executor = criar_executor();
    let config = Configuracao::nova(pequeno.clone(), saida.clone(), TAMANHO_MAXIMO_PADRAO_BYTES)
        .expect("config");
    let cancelamento = Arc::new(AtomicBool::new(false));
    let progresso = GerenciadorProgresso::novo(1, true);

    let relatorio = dividir_arquivo_video(&pequeno, &config, &executor, &progresso, &cancelamento)
        .expect("vídeo pequeno deve retornar sucesso como ignorado");

    assert!(matches!(
        relatorio.status,
        StatusProcessamentoVideo::IgnoradoAbaixoDoLimite {
            limite_bytes: TAMANHO_MAXIMO_PADRAO_BYTES
        }
    ));
    assert!(!saida.join("pequeno").exists());
}

/// 7. Teste de recuo automático (backoff) para keyframe anterior quando alvo == limite máximo.
#[test]
fn recua_para_keyframe_anterior_quando_segmento_ultrapassa_limite() {
    let dir = TempDir::new().expect("tempdir");
    let video = dir.path().join("ajuste_fino.mp4");
    let saida = dir.path().join("saida");

    gerar_video_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=480x270:rate=25:duration=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=10",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v",
            "1600k",
            "-g",
            "25",
            "-keyint_min",
            "25",
            "-sc_threshold",
            "0",
            "-c:a",
            "aac",
        ],
        &video,
    );

    let tam_orig = fs::metadata(&video).expect("meta").len();
    let limite = (tam_orig / 3).max(400_000);

    let executor = criar_executor();
    let mut config = Configuracao::nova(video.clone(), saida, limite).expect("config");
    // Força margem zero entre alvo e limite para estressar o algoritmo de verificação e recuo.
    config.tamanho_alvo_bytes = limite;

    let cancelamento = Arc::new(AtomicBool::new(false));
    let progresso = GerenciadorProgresso::novo(1, true);

    let relatorio = dividir_arquivo_video(&video, &config, &executor, &progresso, &cancelamento)
        .expect("mesmo com alvo apertado, algoritmo deve recuar e convergir");

    match relatorio.status {
        StatusProcessamentoVideo::Dividido { segmentos, .. } => {
            assert!(segmentos.len() >= 3);
            for seg in &segmentos {
                assert!(
                    seg.tamanho_bytes <= limite,
                    "Nenhum segmento pode ultrapassar o limite ({} > {})",
                    seg.tamanho_bytes,
                    limite
                );
            }
        }
        StatusProcessamentoVideo::IgnoradoAbaixoDoLimite { .. } => {
            panic!("Não deveria ser ignorado");
        }
    }
}

/// Teste de processamento paralelo em lote com múltiplos formatos (MP4, MKV, MOV, AVI, WEBM),
/// verificando também a proteção contra sobrescrita não autorizada.
#[test]
fn processa_multiplos_formatos_em_paralelo_e_protege_contra_sobrescrita() {
    let dir = TempDir::new().expect("tempdir");
    let entrada_dir = dir.path().join("videos");
    let saida_dir = dir.path().join("saida");
    fs::create_dir_all(&entrada_dir).expect("criar dir entrada");

    let formatos: Vec<(&str, Vec<&str>)> = vec![
        (
            "clip_mp4.mp4",
            vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=8",
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:d=8",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-b:v",
                "1200k",
                "-g",
                "25",
                "-keyint_min",
                "25",
                "-sc_threshold",
                "0",
                "-c:a",
                "aac",
            ],
        ),
        (
            "clip_mkv.mkv",
            vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=8",
                "-f",
                "lavfi",
                "-i",
                "sine=f=500:d=8",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-b:v",
                "1200k",
                "-g",
                "25",
                "-keyint_min",
                "25",
                "-sc_threshold",
                "0",
                "-c:a",
                "libopus",
            ],
        ),
        (
            "clip_mov.mov",
            vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=8",
                "-f",
                "lavfi",
                "-i",
                "sine=f=600:d=8",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-b:v",
                "1200k",
                "-g",
                "25",
                "-keyint_min",
                "25",
                "-sc_threshold",
                "0",
                "-c:a",
                "aac",
            ],
        ),
        (
            "clip_avi.avi",
            vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=8",
                "-f",
                "lavfi",
                "-i",
                "sine=f=700:d=8",
                "-c:v",
                "mpeg4",
                "-b:v",
                "1200k",
                "-g",
                "25",
                "-c:a",
                "libmp3lame",
            ],
        ),
        (
            "clip_webm.webm",
            vec![
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25:duration=8",
                "-f",
                "lavfi",
                "-i",
                "sine=f=800:d=8",
                "-c:v",
                "libvpx",
                "-b:v",
                "1200k",
                "-g",
                "25",
                "-keyint_min",
                "25",
                "-c:a",
                "libopus",
            ],
        ),
    ];

    let mut caminhos: Vec<PathBuf> = Vec::new();
    for (nome, args) in &formatos {
        let dest = entrada_dir.join(nome);
        gerar_video_ffmpeg(args, &dest);
        caminhos.push(dest);
    }

    let limite = 450_000u64;
    let executor = criar_executor();
    let mut config =
        Configuracao::nova(entrada_dir.clone(), saida_dir.clone(), limite).expect("config lote");
    config.concorrencia = 3;

    let cancelamento = Arc::new(AtomicBool::new(false));
    let resumo = executar_lote(&config, &executor, &cancelamento).expect("executar lote");

    assert_eq!(resumo.itens.len(), 5);
    assert_eq!(
        resumo.quantidade_falhas(),
        0,
        "Todos os 5 formatos devem ser divididos sem erro: {resumo:?}"
    );
    assert_eq!(resumo.quantidade_divididos(), 5);

    for item in &resumo.itens {
        if let ResultadoItemLote::Sucesso(rel) = item
            && let StatusProcessamentoVideo::Dividido { segmentos, .. } = &rel.status
        {
            assert!(segmentos.len() >= 2);
            for seg in segmentos {
                assert!(seg.tamanho_bytes <= limite);
                assert!(seg.inicia_em_keyframe);
            }
        }
    }

    // Reexecutar sem `--sobrescrever` deve falhar com `SobrescritaNaoAutorizada`.
    let resumo_bloqueado =
        executar_lote(&config, &executor, &cancelamento).expect("reexecução bloqueada");
    assert_eq!(resumo_bloqueado.quantidade_falhas(), 5);
    for item in &resumo_bloqueado.itens {
        assert!(matches!(
            item,
            ResultadoItemLote::Falha {
                erro: ErroDivisor::SobrescritaNaoAutorizada { .. },
                ..
            }
        ));
    }

    // Habilitando `--sobrescrever`, deve concluir com sucesso.
    config.permitir_sobrescrita = true;
    let resumo_sobrescrito =
        executar_lote(&config, &executor, &cancelamento).expect("reexecução autorizada");
    assert_eq!(resumo_sobrescrito.quantidade_falhas(), 0);
    assert_eq!(resumo_sobrescrito.quantidade_divididos(), 5);
}

/// 9. Verifica cancelamento seguro sem deixar arquivos temporários residuais.
#[test]
fn cancelamento_seguro_nao_deixa_arquivos_incompletos() {
    let dir = TempDir::new().expect("tempdir");
    let video = dir.path().join("cancelavel.mp4");
    let saida = dir.path().join("saida");

    gerar_video_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=4",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v",
            "500k",
        ],
        &video,
    );

    let executor = criar_executor();
    let config =
        Configuracao::nova(video.clone(), saida.clone(), TAMANHO_ALVO_PADRAO_BYTES).expect("cfg");
    let cancelamento = Arc::new(AtomicBool::new(true));
    let progresso = GerenciadorProgresso::novo(1, true);

    let resultado = dividir_arquivo_video(&video, &config, &executor, &progresso, &cancelamento);
    assert!(matches!(resultado, Err(ErroDivisor::CanceladoPeloUsuario)));
    assert!(!saida.exists());
}

/// 10. Vídeo com intervalos longos entre keyframes (1 keyframe a cada 5s) divisível dentro do limite.
#[test]
fn divide_video_com_intervalos_longos_entre_keyframes_quando_divisivel() {
    let dir = TempDir::new().expect("tempdir");
    let video = dir.path().join("keyframes_espacados.mp4");
    let saida = dir.path().join("saida");

    // 15 segundos com 1 keyframe a cada 5s (125 frames a 25fps) = 3 blocos GOP longos.
    gerar_video_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=480x270:rate=25:duration=15",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=15",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v",
            "1000k",
            "-g",
            "125",
            "-keyint_min",
            "125",
            "-sc_threshold",
            "0",
            "-c:a",
            "aac",
        ],
        &video,
    );

    let tamanho_orig = fs::metadata(&video).expect("meta").len();
    // Limite que permite 1 GOP de 5s (~700 KB) mas não 2 GOPs juntos (~1.4 MB).
    let limite = (tamanho_orig / 2).max(900_000);

    let executor = criar_executor();
    let config = Configuracao::nova(video.clone(), saida, limite).expect("config");
    let cancelamento = Arc::new(AtomicBool::new(false));
    let progresso = GerenciadorProgresso::novo(1, true);
    let mapa_orig = analisar_video(&executor, &video, &cancelamento).expect("analisar");
    assert_eq!(mapa_orig.blocos_gop.len(), 3);

    let relatorio = dividir_arquivo_video(&video, &config, &executor, &progresso, &cancelamento)
        .expect("deve dividir nos keyframes espaçados");

    match relatorio.status {
        StatusProcessamentoVideo::Dividido {
            segmentos,
            validacao,
            ..
        } => {
            assert_eq!(segmentos.len(), 3);
            for seg in &segmentos {
                assert!(seg.tamanho_bytes <= limite);
                assert!(seg.inicia_em_keyframe);
                assert!(
                    (seg.duracao_segundos - 5.0).abs() < 0.35,
                    "Cada parte deve corresponder a 1 GOP de ~5s (obtido {:.2}s)",
                    seg.duracao_segundos
                );
            }
            assert_eq!(
                validacao.total_pacotes_video_segmentos,
                mapa_orig.total_pacotes_video
            );
        }
        StatusProcessamentoVideo::IgnoradoAbaixoDoLimite { .. } => {
            panic!("Não deveria ignorar vídeo acima do limite");
        }
    }
}
