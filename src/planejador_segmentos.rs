//! Algoritmo dinâmico de planejamento de pontos de corte alinhados a keyframes.
//!
//! Considera pacotes de vídeo, todas as faixas de áudio, sobrecarga do contêiner,
//! variações de bitrate (CBR e VBR) e calibração adaptativa baseada no tamanho real
//! dos segmentos gerados.

use std::collections::HashMap;

use crate::analisador_video::{BlocoGop, MapaVideo};
use crate::erros::{ErroDivisor, Resultado};

/// Estimador adaptativo da sobrecarga (overhead) introduzida pelo contêiner em cada segmento.
#[derive(Debug, Clone)]
pub struct EstimadorOverhead {
    /// Overhead fixo estimado por arquivo gerado (cabeçalhos ftyp/moov/EBML/RIFF).
    pub overhead_fixo_bytes: u64,
    /// Overhead variável estimado por pacote (tabelas de índice stsz/stco/cues/idx1).
    pub overhead_por_pacote: f64,
    /// Fator multiplicativo de segurança aplicado sobre a soma de payload (mínimo 1.0).
    pub fator_seguranca_payload: f64,
}

impl EstimadorOverhead {
    /// Cria um estimador inicial baseado nas estatísticas observadas no arquivo original.
    pub fn a_partir_mapa(mapa: &MapaVideo) -> Self {
        Self {
            overhead_fixo_bytes: mapa.overhead_fixo_estimado,
            overhead_por_pacote: mapa.overhead_por_pacote_estimado,
            fator_seguranca_payload: 1.003,
        }
    }

    /// Estima o tamanho total em bytes de um segmento contendo `bytes_pacotes` e `total_pacotes`.
    pub fn estimar_bytes(&self, bytes_pacotes: u64, total_pacotes: u64) -> u64 {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let payload_ajustado = (bytes_pacotes as f64 * self.fator_seguranca_payload).ceil() as u64;

        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let overhead_variavel = (total_pacotes as f64 * self.overhead_por_pacote).ceil() as u64;

        payload_ajustado
            .saturating_add(self.overhead_fixo_bytes)
            .saturating_add(overhead_variavel)
    }

    /// Calibra o estimador usando uma medição real de arquivo gerado pelo FFmpeg.
    ///
    /// Caso o arquivo real tenha sido maior do que o previsto, eleva conservadoramente
    /// os parâmetros de overhead para que os próximos planejamentos acertem sem estourar.
    pub fn calibrar_com_medicao(
        &mut self,
        bytes_pacotes_estimados: u64,
        total_pacotes: u64,
        tamanho_real_bytes: u64,
    ) {
        let estimado_anterior = self.estimar_bytes(bytes_pacotes_estimados, total_pacotes);
        if tamanho_real_bytes <= estimado_anterior {
            return;
        }

        let overhead_real = tamanho_real_bytes.saturating_sub(bytes_pacotes_estimados);
        let diferenca = tamanho_real_bytes.saturating_sub(estimado_anterior);

        // Incorpora o excedente observado distribuindo entre overhead fixo e por pacote.
        let metade = diferenca / 2;
        self.overhead_fixo_bytes = self.overhead_fixo_bytes.saturating_add(metade.max(8_192));

        if total_pacotes > 0 {
            #[allow(clippy::cast_precision_loss)]
            let observado_por_pacote = overhead_real as f64 / total_pacotes as f64;
            if observado_por_pacote > self.overhead_por_pacote {
                self.overhead_por_pacote = observado_por_pacote.min(1024.0);
            }
        }
    }
}

/// Especificação planejada para um segmento individual de saída.
#[derive(Debug, Clone)]
pub struct SegmentoPlanejado {
    /// Número do segmento (base 1: 1, 2, 3...).
    pub indice: usize,
    /// Índice inclusivo do primeiro bloco GOP deste segmento.
    pub bloco_inicio: usize,
    /// Índice exclusivo do último bloco GOP deste segmento.
    pub bloco_fim_exclusivo: usize,
    /// Timestamp relativo (em segundos) do início deste segmento.
    pub tempo_inicio_relativo: f64,
    /// Timestamp relativo (em segundos) do fim deste segmento (ou `None` se for até o final).
    pub tempo_fim_relativo: Option<f64>,
    /// Soma dos bytes de pacotes (vídeo + áudio + outros) nos blocos planejados.
    pub bytes_pacotes: u64,
    /// Total de pacotes (vídeo + áudio) nos blocos planejados.
    pub total_pacotes: u64,
    /// Tamanho estimado do arquivo final em bytes (incluindo overhead do contêiner).
    pub tamanho_estimado_bytes: u64,
}

impl SegmentoPlanejado {
    /// Quantidade de blocos GOP (keyframes) abrangidos por este segmento.
    pub fn quantidade_blocos(&self) -> usize {
        self.bloco_fim_exclusivo.saturating_sub(self.bloco_inicio)
    }
}

/// Plano completo de divisão contendo todos os segmentos e os instantes exatos de corte para o FFmpeg.
#[derive(Debug, Clone)]
pub struct PlanoDivisao {
    /// Lista ordenada e contígua de segmentos cobrindo 100% dos blocos GOP.
    pub segmentos: Vec<SegmentoPlanejado>,
    /// Lista de tempos relativos para `-segment_times` do FFmpeg (sempre `segmentos.len() - 1`).
    pub tempos_corte_ffmpeg: Vec<f64>,
}

/// Planejador dinâmico de segmentos respeitando tamanho-alvo e limite máximo absoluto.
#[derive(Debug, Clone)]
pub struct PlanejadorSegmentos {
    tamanho_maximo_bytes: u64,
    tamanho_alvo_bytes: u64,
    estimador: EstimadorOverhead,
}

impl PlanejadorSegmentos {
    /// Inicializa o planejador para um vídeo analisado e limites configurados.
    pub fn novo(mapa: &MapaVideo, tamanho_maximo_bytes: u64, tamanho_alvo_bytes: u64) -> Self {
        Self {
            tamanho_maximo_bytes,
            tamanho_alvo_bytes: tamanho_alvo_bytes.min(tamanho_maximo_bytes),
            estimador: EstimadorOverhead::a_partir_mapa(mapa),
        }
    }

    /// Acesso mutável ao estimador de overhead para calibração após medições reais.
    pub fn estimador_mut(&mut self) -> &mut EstimadorOverhead {
        &mut self.estimador
    }

    /// Gera um plano completo de divisão cobrindo todos os blocos GOP sem lacunas nem sobreposições.
    ///
    /// `tetos_por_bloco_inicio` permite restringir o bloco final máximo de segmentos que
    /// eventualmente ultrapassaram o limite em uma tentativa anterior (mecanismo de recuo).
    pub fn planejar(
        &self,
        mapa: &MapaVideo,
        tetos_por_bloco_inicio: &HashMap<usize, usize>,
    ) -> Resultado<PlanoDivisao> {
        let blocos = &mapa.blocos_gop;
        if blocos.is_empty() {
            return Err(ErroDivisor::SemKeyframes {
                caminho: mapa.metadados.caminho.clone(),
                detalhe: "Lista de blocos GOP está vazia.".to_string(),
            });
        }

        let tempo_base = blocos[0].pts_inicio;
        let total_blocos = blocos.len();
        let mut segmentos = Vec::new();
        let mut bloco_atual = 0usize;

        // Teto suave apenas para absorver o último bloco restante e evitar parte final minúscula,
        // mantendo ainda metade da margem de segurança em relação ao limite absoluto.
        let margem_disponivel = self
            .tamanho_maximo_bytes
            .saturating_sub(self.tamanho_alvo_bytes);
        let teto_absorcao_final = self
            .tamanho_alvo_bytes
            .saturating_add(margem_disponivel / 2);

        while bloco_atual < total_blocos {
            let primeiro_bloco = &blocos[bloco_atual];

            // Se o payload puro de um único GOP já ultrapassa o limite máximo absoluto,
            // é fisicamente impossível gerar um segmento válido sem recodificar.
            if primeiro_bloco.bytes_totais_pacotes > self.tamanho_maximo_bytes {
                return Err(criar_erro_gop_indivisivel(
                    mapa,
                    primeiro_bloco,
                    primeiro_bloco.bytes_totais_pacotes,
                    self.tamanho_maximo_bytes,
                ));
            }

            let limite_superior = tetos_por_bloco_inicio
                .get(&bloco_atual)
                .copied()
                .unwrap_or(total_blocos)
                .clamp(bloco_atual + 1, total_blocos);

            let mut bytes_acumulados = primeiro_bloco.bytes_totais_pacotes;
            let mut pacotes_acumulados = primeiro_bloco.total_pacotes();
            let mut fim_exclusivo = bloco_atual + 1;

            while fim_exclusivo < limite_superior {
                let proximo = &blocos[fim_exclusivo];
                let novos_bytes = bytes_acumulados.saturating_add(proximo.bytes_totais_pacotes);
                let novos_pacotes = pacotes_acumulados.saturating_add(proximo.total_pacotes());
                let estimativa = self.estimador.estimar_bytes(novos_bytes, novos_pacotes);

                let eh_ultimo_bloco_do_video = fim_exclusivo + 1 == total_blocos;
                let teto_aplicavel = if eh_ultimo_bloco_do_video
                    && !tetos_por_bloco_inicio.contains_key(&bloco_atual)
                {
                    teto_absorcao_final
                } else {
                    self.tamanho_alvo_bytes
                };

                if estimativa <= teto_aplicavel {
                    bytes_acumulados = novos_bytes;
                    pacotes_acumulados = novos_pacotes;
                    fim_exclusivo += 1;
                } else {
                    break;
                }
            }

            let tamanho_estimado = self
                .estimador
                .estimar_bytes(bytes_acumulados, pacotes_acumulados);

            let tempo_inicio_relativo = (primeiro_bloco.pts_inicio - tempo_base).max(0.0);
            let tempo_fim_relativo = if fim_exclusivo < total_blocos {
                Some((blocos[fim_exclusivo].pts_inicio - tempo_base).max(tempo_inicio_relativo))
            } else {
                None
            };

            segmentos.push(SegmentoPlanejado {
                indice: segmentos.len() + 1,
                bloco_inicio: bloco_atual,
                bloco_fim_exclusivo: fim_exclusivo,
                tempo_inicio_relativo,
                tempo_fim_relativo,
                bytes_pacotes: bytes_acumulados,
                total_pacotes: pacotes_acumulados,
                tamanho_estimado_bytes: tamanho_estimado,
            });

            bloco_atual = fim_exclusivo;
        }

        // Calcula os instantes de corte seguros para o muxer `-f segment` do FFmpeg.
        // Para cortar exatamente no keyframe `blocos[k]`, o instante deve ficar estritamente
        // entre o keyframe anterior `blocos[k - 1]` e `min(pts(k), dts(k))`.
        let mut tempos_corte_ffmpeg = Vec::with_capacity(segmentos.len().saturating_sub(1));
        for seg in segmentos.iter().skip(1) {
            let tempo_corte = calcular_instante_corte_seguro(blocos, seg.bloco_inicio, tempo_base);
            tempos_corte_ffmpeg.push(tempo_corte);
        }

        Ok(PlanoDivisao {
            segmentos,
            tempos_corte_ffmpeg,
        })
    }

    /// Determina um ponto de corte anterior (`novo_fim_exclusivo`) quando um segmento gerado
    /// ultrapassa o limite máximo configurado.
    ///
    /// Caso o segmento já contenha apenas um único bloco GOP, retorna
    /// `ErroDivisor::DivisaoImpossivelSemRecodificacao`.
    pub fn calcular_recuo_bloco(
        &self,
        mapa: &MapaVideo,
        segmento: &SegmentoPlanejado,
        tamanho_real_bytes: u64,
    ) -> Resultado<usize> {
        if segmento.quantidade_blocos() <= 1 {
            let bloco = &mapa.blocos_gop[segmento.bloco_inicio];
            return Err(criar_erro_gop_indivisivel(
                mapa,
                bloco,
                tamanho_real_bytes,
                self.tamanho_maximo_bytes,
            ));
        }

        // Quanto precisamos reduzir para voltar confortavelmente abaixo do tamanho-alvo.
        let excesso_bytes = tamanho_real_bytes.saturating_sub(self.tamanho_alvo_bytes);
        let mut bytes_removidos = 0u64;
        let mut novo_fim = segmento.bloco_fim_exclusivo;

        while novo_fim > segmento.bloco_inicio + 1 {
            novo_fim -= 1;
            bytes_removidos =
                bytes_removidos.saturating_add(mapa.blocos_gop[novo_fim].bytes_totais_pacotes);
            if bytes_removidos >= excesso_bytes {
                break;
            }
        }

        // Garante recuo estritamente menor que o fim anterior.
        let resultado = novo_fim
            .min(segmento.bloco_fim_exclusivo.saturating_sub(1))
            .max(segmento.bloco_inicio + 1);

        Ok(resultado)
    }
}

/// Calcula o timestamp relativo seguro para o FFmpeg `-segment_times` cortar exatamente
/// no keyframe `indice_bloco_corte` (e nunca no keyframe anterior ou posterior),
/// inclusive na presença de B-frames (onde `dts < pts`).
pub fn calcular_instante_corte_seguro(
    blocos: &[BlocoGop],
    indice_bloco_corte: usize,
    tempo_base: f64,
) -> f64 {
    let atual = &blocos[indice_bloco_corte];
    let anterior = &blocos[indice_bloco_corte.saturating_sub(1)];

    let pts_anterior = (anterior.pts_inicio - tempo_base).max(0.0);
    let dts_anterior = (anterior.dts_inicio - tempo_base).max(0.0);
    let limite_inferior = pts_anterior.max(dts_anterior);

    let pts_atual = (atual.pts_inicio - tempo_base).max(0.0);
    let dts_atual = (atual.dts_inicio - tempo_base).max(0.0);
    let limite_superior = if dts_atual > limite_inferior {
        pts_atual.min(dts_atual)
    } else {
        pts_atual
    };

    if limite_superior <= limite_inferior {
        return pts_atual;
    }

    let intervalo = limite_superior - limite_inferior;
    let margem = (intervalo * 0.25).min(0.05);
    (limite_superior - margem).max(limite_inferior + intervalo * 0.5)
}

/// Constrói mensagem explicativa e acionável quando um único intervalo entre keyframes
/// excede o limite máximo e não pode ser dividido sem recodificação.
fn criar_erro_gop_indivisivel(
    mapa: &MapaVideo,
    bloco: &BlocoGop,
    tamanho_observado: u64,
    limite_maximo: u64,
) -> ErroDivisor {
    let tempo_base = mapa.blocos_gop.first().map_or(0.0, |b| b.pts_inicio);
    let inicio_rel = (bloco.pts_inicio - tempo_base).max(0.0);
    let fim_rel = (bloco.pts_fim - tempo_base).max(inicio_rel);
    let motivo = format!(
        "o intervalo indivisível entre keyframes #{indice} ({inicio_rel:.2}s a {fim_rel:.2}s, duração {duracao:.2}s) ocupa {tamanho_observado} bytes e não possui keyframes internos para um corte anterior",
        indice = bloco.indice + 1,
        duracao = bloco.duracao_segundos,
    );

    ErroDivisor::DivisaoImpossivelSemRecodificacao {
        caminho: mapa.metadados.caminho.clone(),
        motivo,
        tamanho_minimo_bytes: tamanho_observado,
        limite_maximo_bytes: limite_maximo,
    }
}

#[cfg(test)]
mod testes {
    use std::path::PathBuf;

    use super::*;
    use crate::analisador_video::{InfoStreamVideo, MetadadosVideo};

    fn criar_mapa_teste(tamanhos_blocos: &[u64]) -> MapaVideo {
        let mut blocos = Vec::new();
        let mut soma = 0u64;
        for (i, &tam) in tamanhos_blocos.iter().enumerate() {
            soma += tam;
            #[allow(clippy::cast_precision_loss)]
            let inicio = i as f64 * 2.0;
            blocos.push(BlocoGop {
                indice: i,
                pts_inicio: inicio,
                dts_inicio: (inicio - 0.066).max(0.0),
                pts_fim: inicio + 2.0,
                duracao_segundos: 2.0,
                bytes_video: (tam * 9) / 10,
                bytes_audio: tam - (tam * 9) / 10,
                bytes_outros: 0,
                bytes_totais_pacotes: tam,
                quantidade_pacotes_video: 60,
                quantidade_pacotes_audio: 86,
            });
        }

        let metadados = MetadadosVideo {
            caminho: PathBuf::from("video_teste.mp4"),
            formato_nome: "mov,mp4,m4a,3gp,3g2,mj2".to_string(),
            tamanho_bytes: soma + 50_000,
            #[allow(clippy::cast_precision_loss)]
            duracao_segundos: tamanhos_blocos.len() as f64 * 2.0,
            tempo_inicial_formato: 0.0,
            bitrate_geral: Some(8_000_000),
            indice_stream_video_principal: 0,
            streams_video: vec![InfoStreamVideo {
                indice: 0,
                codec_nome: "h264".to_string(),
                codec_tag: Some("avc1".to_string()),
                largura: 1920,
                altura: 1080,
                formato_pixel: Some("yuv420p".to_string()),
                fps_racional: "30/1".to_string(),
                fps: 30.0,
                bitrate: Some(7_500_000),
                sar: Some("1:1".to_string()),
                dar: Some("16:9".to_string()),
            }],
            streams_audio: vec![],
            indices_streams_mapeados: vec![0],
        };

        MapaVideo {
            metadados,
            blocos_gop: blocos,
            soma_bytes_pacotes: soma,
            total_pacotes_video: tamanhos_blocos.len() as u64 * 60,
            total_pacotes_audio: tamanhos_blocos.len() as u64 * 86,
            overhead_total_observado: 50_000,
            overhead_por_pacote_estimado: 16.0,
            overhead_fixo_estimado: 24_576,
            eh_vbr: false,
        }
    }

    #[test]
    fn planeja_cbr_com_aproveitamento_e_sem_lacunas() {
        // 10 blocos de 2.4 MB cada = 24 MB total -> deve agrupar 4 blocos (~9.6 MB), 4 blocos, 2 blocos
        let mapa = criar_mapa_teste(&[2_400_000; 10]);
        let planejador = PlanejadorSegmentos::novo(&mapa, 10_000_000, 9_800_000);
        let plano = planejador.planejar(&mapa, &HashMap::new());
        assert!(plano.is_ok());

        if let Ok(plano) = plano {
            assert_eq!(plano.segmentos.len(), 3);
            assert_eq!(plano.tempos_corte_ffmpeg.len(), 2);
            assert_eq!(plano.segmentos[0].bloco_inicio, 0);
            assert_eq!(plano.segmentos[0].bloco_fim_exclusivo, 4);
            assert_eq!(plano.segmentos[1].bloco_inicio, 4);
            assert_eq!(plano.segmentos[1].bloco_fim_exclusivo, 8);
            assert_eq!(plano.segmentos[2].bloco_inicio, 8);
            assert_eq!(plano.segmentos[2].bloco_fim_exclusivo, 10);

            for seg in &plano.segmentos {
                assert!(seg.tamanho_estimado_bytes <= 10_000_000);
            }
        }
    }

    #[test]
    fn planeja_vbr_dinamicamente() {
        // Blocos com forte variação de tamanho (cenas estáticas vs explosão de movimento)
        let mapa = criar_mapa_teste(&[
            500_000, 600_000, 4_500_000, 3_800_000, 5_000_000, 4_200_000, 700_000,
        ]);
        let planejador = PlanejadorSegmentos::novo(&mapa, 10_000_000, 9_800_000);
        let plano = planejador.planejar(&mapa, &HashMap::new());
        assert!(plano.is_ok());

        if let Ok(plano) = plano {
            // Verifica cobertura contígua completa de 0 até 7
            let mut esperado = 0;
            for seg in &plano.segmentos {
                assert_eq!(seg.bloco_inicio, esperado);
                esperado = seg.bloco_fim_exclusivo;
                assert!(seg.tamanho_estimado_bytes <= 10_000_000);
            }
            assert_eq!(esperado, 7);
        }
    }

    #[test]
    fn detecta_gop_unico_maior_que_limite_e_informa_erro_claro() {
        let mapa = criar_mapa_teste(&[3_000_000, 12_500_000, 2_000_000]);
        let planejador = PlanejadorSegmentos::novo(&mapa, 10_000_000, 9_800_000);
        let resultado = planejador.planejar(&mapa, &HashMap::new());
        assert!(matches!(
            resultado,
            Err(ErroDivisor::DivisaoImpossivelSemRecodificacao {
                tamanho_minimo_bytes: 12_500_000,
                limite_maximo_bytes: 10_000_000,
                ..
            })
        ));
    }

    #[test]
    fn recuo_de_bloco_reduz_segmento_estourado() {
        let mapa = criar_mapa_teste(&[2_400_000; 6]);
        let planejador = PlanejadorSegmentos::novo(&mapa, 10_000_000, 9_800_000);
        let plano = planejador.planejar(&mapa, &HashMap::new());
        assert!(plano.is_ok());
        if let Ok(plano) = plano {
            let primeiro = &plano.segmentos[0];
            assert_eq!(primeiro.bloco_fim_exclusivo, 4);
            let novo_fim = planejador.calcular_recuo_bloco(&mapa, primeiro, 10_300_000);
            assert!(novo_fim.is_ok());
            if let Ok(fim) = novo_fim {
                assert!(fim < primeiro.bloco_fim_exclusivo);
                assert!(fim > primeiro.bloco_inicio);
            }
        }
    }

    #[test]
    fn instante_corte_respeita_bframes_entre_keyframes() {
        let mapa = criar_mapa_teste(&[1_000_000; 4]);
        let corte = calcular_instante_corte_seguro(&mapa.blocos_gop, 2, 0.0);
        // Keyframe 1 está em 2.0s; keyframe 2 tem pts=4.0s e dts=3.934s
        assert!(corte > 2.0);
        assert!(corte < 3.934);
    }
}
