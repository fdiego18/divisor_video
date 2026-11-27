//! Biblioteca do `divisor_video` para divisão automática de arquivos de vídeo
//! sem recodificação (`-c copy`), respeitando rigorosamente o limite máximo de tamanho.
//!
//! O núcleo de processamento e os módulos de serviços permanecem completamente
//! independentes de `egui` e de qualquer biblioteca gráfica.

// --- Núcleo de processamento (independente de GUI) ---
pub mod analisador_video;
pub mod configuracao;
pub mod divisor_video;
pub mod erros;
pub mod executor_ffmpeg;
pub mod ferramentas_distribuidas;
pub mod planejador_segmentos;
pub mod processamento_lote;
pub mod progresso;
pub mod validador;

// --- Serviços desktop (independentes de GUI) ---
pub mod configuracoes_persistentes;
pub mod eventos;
pub mod fila_processamento;
pub mod historico;
pub mod integracao_so;
pub mod miniaturas;
pub mod notificacoes;

// --- Interface gráfica desktop (isolada pela feature `gui`) ---
#[cfg(feature = "gui")]
pub mod gui;
