//! Ponto de entrada da interface de linha de comando (CLI) do `divisor_video`.

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use tracing::{error, warn};
use tracing_subscriber::EnvFilter;

use divisor_video::configuracao::{ArgumentosCli, Configuracao};
use divisor_video::executor_ffmpeg::ExecutorFfmpeg;
use divisor_video::processamento_lote::{executar_lote, imprimir_resumo_lote};

fn main() -> ExitCode {
    let args = ArgumentosCli::parse();
    inicializar_logging(args.verboso);

    let config = match Configuracao::a_partir_cli(args) {
        Ok(cfg) => cfg,
        Err(erro) => {
            eprintln!("Erro de configuração: {erro}");
            return ExitCode::from(2);
        }
    };

    let executor = match ExecutorFfmpeg::detectar(
        config.caminho_ffmpeg.clone(),
        config.caminho_ffprobe.clone(),
    ) {
        Ok(exec) => exec,
        Err(erro) => {
            eprintln!("Erro de ambiente: {erro}");
            return ExitCode::from(3);
        }
    };

    let cancelamento = Arc::new(AtomicBool::new(false));
    registrar_manipulador_cancelamento(&cancelamento);

    match executar_lote(&config, &executor, &cancelamento) {
        Ok(resumo) => {
            imprimir_resumo_lote(&resumo, config.tamanho_maximo_bytes);

            if resumo.cancelado {
                eprintln!(
                    "\nExecução interrompida pelo usuário. Arquivos temporários foram limpos."
                );
                ExitCode::from(130)
            } else if resumo.quantidade_falhas() > 0 {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(erro) => {
            error!(%erro, "Erro fatal durante o processamento");
            eprintln!("Erro: {erro}");
            ExitCode::from(1)
        }
    }
}

/// Configura o assinante `tracing` direcionado ao `stderr` respeitando `RUST_LOG` e `--verboso`.
fn inicializar_logging(verboso: bool) {
    let filtro_padrao = if verboso {
        "divisor_video=debug"
    } else {
        "divisor_video=warn"
    };

    let filtro =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filtro_padrao));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filtro)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

/// Registra manipulador de sinal Ctrl+C (SIGINT) para cancelamento gracioso e seguro.
fn registrar_manipulador_cancelamento(cancelamento: &Arc<AtomicBool>) {
    let sinal = Arc::clone(cancelamento);
    if let Err(erro) = ctrlc::set_handler(move || {
        let ja_solicitado = sinal.swap(true, Ordering::SeqCst);
        if !ja_solicitado {
            warn!("Sinal de cancelamento recebido (Ctrl+C). Encerrando processos com segurança...");
            eprintln!("\nCancelamento solicitado. Finalizando tarefas e removendo temporários...");
        }
    }) {
        warn!(%erro, "Não foi possível registrar o manipulador de Ctrl+C");
    }
}
