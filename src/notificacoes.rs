//! Gerenciamento de notificações visuais (toasts) e notificações opcionais do sistema operacional.

use std::time::{Duration, Instant};

use crate::configuracoes_persistentes::PreferenciasNotificacoes;

/// Limite máximo de toasts visíveis simultaneamente na interface.
const MAX_NOTIFICACOES_VISIVEIS: usize = 5;

/// Categoria funcional de uma notificação emitida pelo aplicativo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoriaNotificacao {
    ProcessamentoConcluido,
    ProcessamentoComFalhas,
    FilaFinalizada,
    FaltaEspacoDisco,
    Cancelamento,
    DependenciasIndisponiveis,
    Informativa,
}

impl CategoriaNotificacao {
    /// Verifica se a categoria está habilitada nas preferências do usuário.
    pub fn esta_habilitada(self, prefs: &PreferenciasNotificacoes) -> bool {
        match self {
            Self::ProcessamentoConcluido => prefs.processamento_concluido,
            Self::ProcessamentoComFalhas => prefs.processamento_com_falhas,
            Self::FilaFinalizada => prefs.fila_finalizada,
            Self::FaltaEspacoDisco => prefs.falta_espaco_disco,
            Self::Cancelamento => prefs.cancelamento,
            Self::DependenciasIndisponiveis => prefs.dependencias_indisponiveis,
            Self::Informativa => true,
        }
    }
}

/// Alerta visual temporário exibido na interface gráfica.
#[derive(Debug, Clone)]
pub struct NotificacaoVisual {
    pub id: u64,
    pub titulo: String,
    pub mensagem: String,
    pub categoria: CategoriaNotificacao,
    pub criada_em: Instant,
    pub duracao: Duration,
}

impl NotificacaoVisual {
    /// Indica se o tempo de exibição do alerta já expirou.
    pub fn expirou(&self) -> bool {
        self.criada_em.elapsed() >= self.duracao
    }
}

/// Central responsável por filtrar, armazenar toasts ativos e disparar notificações nativas.
#[derive(Debug, Default)]
pub struct CentralNotificacoes {
    proximo_id: u64,
    ativas: Vec<NotificacaoVisual>,
}

impl CentralNotificacoes {
    /// Cria uma nova central de notificações vazia.
    pub fn nova() -> Self {
        Self::default()
    }

    /// Emite uma notificação respeitando rigorosamente as preferências do usuário.
    pub fn emitir(
        &mut self,
        categoria: CategoriaNotificacao,
        titulo: impl Into<String>,
        mensagem: impl Into<String>,
        prefs: &PreferenciasNotificacoes,
    ) {
        if !categoria.esta_habilitada(prefs) {
            return;
        }

        let titulo_str = titulo.into();
        let mensagem_str = mensagem.into();
        self.proximo_id = self.proximo_id.wrapping_add(1);

        let duracao = match categoria {
            CategoriaNotificacao::FaltaEspacoDisco
            | CategoriaNotificacao::DependenciasIndisponiveis
            | CategoriaNotificacao::ProcessamentoComFalhas => Duration::from_secs(9),
            _ => Duration::from_secs(6),
        };

        self.ativas.push(NotificacaoVisual {
            id: self.proximo_id,
            titulo: titulo_str.clone(),
            mensagem: mensagem_str.clone(),
            categoria,
            criada_em: Instant::now(),
            duracao,
        });

        while self.ativas.len() > MAX_NOTIFICACOES_VISIVEIS {
            self.ativas.remove(0);
        }

        if prefs.habilitar_notificacoes_sistema
            && !matches!(categoria, CategoriaNotificacao::Informativa)
        {
            disparar_notificacao_sistema_em_segundo_plano(titulo_str, mensagem_str);
        }
    }

    /// Remove notificações cujo prazo já expirou.
    pub fn limpar_expiradas(&mut self) {
        self.ativas.retain(|n| !n.expirou());
    }

    /// Remove manualmente uma notificação pelo seu ID.
    pub fn descartar(&mut self, id: u64) {
        self.ativas.retain(|n| n.id != id);
    }

    /// Lista das notificações atualmente visíveis.
    pub fn ativas(&self) -> &[NotificacaoVisual] {
        &self.ativas
    }
}

/// Dispara uma notificação nativa do sistema operacional em uma thread separada
/// para nunca bloquear a interface gráfica (falhas em ambientes sem daemon são ignoradas).
#[cfg(feature = "gui")]
fn disparar_notificacao_sistema_em_segundo_plano(titulo: String, mensagem: String) {
    let _ = std::thread::Builder::new()
        .name("notificacao-so".to_string())
        .spawn(move || {
            let _ = notify_rust::Notification::new()
                .summary(&titulo)
                .body(&mensagem)
                .appname("Divisor de Vídeo")
                .timeout(notify_rust::Timeout::Milliseconds(5000))
                .show();
        });
}

#[cfg(not(feature = "gui"))]
fn disparar_notificacao_sistema_em_segundo_plano(_titulo: String, _mensagem: String) {}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn respeita_preferencias_e_limite_de_toasts() {
        let mut central = CentralNotificacoes::nova();
        let prefs = PreferenciasNotificacoes {
            habilitar_notificacoes_sistema: false,
            processamento_concluido: false,
            fila_finalizada: true,
            ..Default::default()
        };

        central.emitir(
            CategoriaNotificacao::ProcessamentoConcluido,
            "Ignorada",
            "Não deve aparecer",
            &prefs,
        );
        assert!(central.ativas().is_empty());

        for i in 0..8 {
            central.emitir(
                CategoriaNotificacao::FilaFinalizada,
                format!("Aviso {i}"),
                "Concluído",
                &prefs,
            );
        }
        assert_eq!(central.ativas().len(), MAX_NOTIFICACOES_VISIVEIS);
    }
}
