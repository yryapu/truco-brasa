//! A visão que **um** assento tem da partida.
//!
//! Esta é a peça de segurança do jogo, e é por isso que ela mora no crate de regras, com
//! teste, em vez de ser montada à mão no servidor: o invariante é que a visão de um assento
//! nunca contém carta que aquele assento não tem direito de ver — nem a mão alheia, nem a
//! carta jogada de costas (R-14), nem o resto do monte.
//!
//! A única exceção é a da própria regra: na mão de onze a dupla vê as cartas entre si (R-25).

use crate::carta::Carta;
use crate::partida::{Modo, Partida, Pendencia, TipoMao, equipe_de};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct JogadaVisivel {
    pub assento: usize,
    /// `None` quando a carta foi jogada de costas: nem o valor, nem o naipe.
    pub carta: Option<Carta>,
    pub coberta: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Visao {
    pub assento: usize,
    pub equipe: u8,
    pub modo: Modo,
    pub placar: [u8; 2],
    pub numero_da_mao: u32,
    pub vira: Carta,
    /// O número das manilhas desta mão, como caractere de carta de paus (só para a interface
    /// mostrar "a manilha é o 6"); a força real vem do servidor.
    pub manilha: char,
    pub minhas_cartas: Vec<Carta>,
    /// Só preenchido na mão de onze da própria dupla (R-25). `None` no resto do jogo.
    pub cartas_do_parceiro: Option<Vec<Carta>>,
    pub mesa: Vec<JogadaVisivel>,
    /// Vencedora de cada rodada já resolvida; `None` dentro = rodada empatada.
    pub rodadas: Vec<Option<u8>>,
    /// Quantas cartas cada assento ainda tem. Contagem, nunca a carta.
    pub cartas_na_mao: Vec<usize>,
    pub vez: usize,
    pub valor: u8,
    pub tipo: TipoMao,
    pub pendencia: Option<Pendencia>,
    pub aguarda_onze: bool,
    pub vencedora: Option<u8>,
    /// O que este assento pode fazer agora. A interface desenha os botões a partir daqui, e
    /// o servidor valida de novo — a lista é conveniência, não autoridade.
    pub acoes: Vec<&'static str>,
}

impl Visao {
    pub fn para(p: &Partida, assento: usize) -> Visao {
        let m = &p.mao;
        let equipe = equipe_de(assento);
        let minha_onze = matches!(m.tipo, TipoMao::Onze { equipe: e } if e == equipe);

        let cartas_do_parceiro = (minha_onze && p.modo == Modo::DoisContraDois).then(|| {
            let parceiro = (assento + 2) % p.assentos();
            m.cartas[parceiro].clone()
        });

        Visao {
            assento,
            equipe,
            modo: p.modo,
            placar: p.placar,
            numero_da_mao: p.numero_da_mao,
            vira: m.vira,
            manilha: Carta::nova(m.manilha, crate::carta::Naipe::Paus).unicode(),
            minhas_cartas: m.cartas[assento].clone(),
            cartas_do_parceiro,
            mesa: m
                .mesa
                .iter()
                .map(|j| JogadaVisivel {
                    assento: j.assento,
                    carta: (!j.coberta).then_some(j.carta),
                    coberta: j.coberta,
                })
                .collect(),
            rodadas: m.rodadas.iter().map(|r| r.vencedor).collect(),
            cartas_na_mao: m.cartas.iter().map(Vec::len).collect(),
            vez: m.vez,
            valor: m.valor,
            tipo: m.tipo,
            pendencia: m.pendencia,
            aguarda_onze: m.aguarda_onze(),
            vencedora: p.vencedora,
            acoes: acoes_de(p, assento),
        }
    }
}

/// As ações legais para este assento agora. Derivada das mesmas regras que `aplicar` aplica —
/// se as duas discordarem, `aplicar` é quem vale, e isso é um defeito a corrigir aqui.
fn acoes_de(p: &Partida, assento: usize) -> Vec<&'static str> {
    let mut v = Vec::new();
    if p.vencedora.is_some() {
        return v;
    }
    let m = &p.mao;
    if m.aguarda_onze() {
        if matches!(m.tipo, TipoMao::Onze { equipe } if equipe == equipe_de(assento)) {
            v.push("onze_aceitar");
            v.push("onze_correr");
        }
        return v;
    }
    if let Some(pend) = m.pendencia {
        if pend.assento_respondente == assento {
            v.push("aceitar");
            v.push("correr");
            if crate::partida::proximo_valor(pend.valor_proposto).is_some() {
                v.push("aumentar");
            }
        }
        return v;
    }
    if m.vez == assento {
        v.push("jogar");
        if !m.rodadas.is_empty() {
            v.push("jogar_coberta");
        }
        let pode_pedir = matches!(m.tipo, TipoMao::Normal)
            && crate::partida::proximo_valor(m.valor).is_some()
            && m.ultima_equipe_pedinte != Some(equipe_de(assento));
        if pode_pedir {
            v.push("pedir");
        }
    }
    v
}
