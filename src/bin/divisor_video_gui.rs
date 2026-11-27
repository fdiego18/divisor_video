#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

//! Ponto de entrada do aplicativo desktop gráfico `divisor_video_gui` (egui / eframe).

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let filtro = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("divisor_video=info,eframe=warn,wgpu=error"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filtro)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();

    match divisor_video::gui::executar_aplicativo_desktop() {
        Ok(()) => ExitCode::SUCCESS,
        Err(erro) => {
            eprintln!("Erro ao iniciar a interface gráfica do Divisor de Vídeo: {erro}");
            ExitCode::FAILURE
        }
    }
}
