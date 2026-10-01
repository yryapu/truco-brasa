//! O bot que preenche mesa de treino.
//!
//! **O bot é um cliente como qualquer outro.** Ele recebe exatamente a mesma `Visao` que um
//! humano no mesmo assento receberia, e responde com as mesmas cinco mensagens. Não há
//! atalho para o estado da partida, nem acesso ao `Partida`, nem um canal privilegiado.
//!
//! Isso não é elegância: é a propriedade que torna o bot **incapaz** de ver a carta do
//! adversário, em vez de apenas não programado para ver. Se amanhã alguém piorar a
//! estratégia, continua impossível trapacear; e o teste de isolamento que vale para o
//! humano vale para ele sem uma linha a mais.

use std::time::Duration;

use rand::{Rng, RngExt};
use tokio::sync::mpsc;
use truco_regras::carta::{Carta, Numero};
use truco_regras::visao::Visao;
use truco_regras::{Acao, equipe_de};

use crate::mesas::{Comando, ParaCliente};

/// Nomes de mesa de bar. O cliente marca quem é bot num selo, então o nome não precisa
/// avisar — e um nome que avisa ("Bot 1") tira a graça da mesa.
pub const NOMES: [&str; 3] = ["Tião", "Zefa", "Maneco"];

pub fn soltar(
    assento: usize,
    mut recebe: mpsc::UnboundedReceiver<ParaCliente>,
    manda: mpsc::UnboundedSender<Comando>,
    // Teto do tempo de "pensar". Zero = age na hora, que é o que o teste quer.
    pausa_maxima: Duration,
) {
    tokio::spawn(async move {
        while let Some(msg) = recebe.recv().await {
            // Só o estado interessa: é dele que sai toda decisão. `avisos` é narração para
            // humano, e `erro` só chega se o bot tentou algo ilegal — o que é defeito a
            // registrar, não algo a contornar.
            let visao = match msg {
                ParaCliente::Estado(v) => v,
                ParaCliente::Erro { erro, mensagem } => {
                    tracing::warn!(assento, erro, %mensagem, "o bot tentou jogada ilegal");
                    continue;
                }
                ParaCliente::Fim { .. } => break,
                _ => continue,
            };
            let Some(acao) = decidir(&visao, &mut rand::rng()) else {
                continue;
            };

            // Metade a inteiro do teto, para dois bots na mesma mesa não agirem em uníssono.
            if !pausa_maxima.is_zero() {
                let minimo = pausa_maxima / 2;
                let pausa = rand::rng().random_range(minimo..=pausa_maxima);
                tokio::time::sleep(pausa).await;
            }
            if manda.send(Comando::Agir(assento, acao)).is_err() {
                break;
            }
        }
    });
}

/// A força de cada carta da mão, já com a manilha desta mão aplicada.
fn forcas(v: &Visao) -> Vec<(usize, u8)> {
    let manilha = Numero::do_rotulo(v.manilha).unwrap_or(Numero::Quatro);
    v.minhas_cartas
        .iter()
        .enumerate()
        .map(|(i, c)| (i, c.forca(manilha)))
        .collect()
}

/// Quantas cartas "de matar" (manilha, 2 ou 3) e quantas "boas" (K ou A) o bot tem.
/// Os cortes vêm da ordem de força: manilha ≥ 10, `3` = 9, `2` = 8, `A` = 7, `K` = 6.
fn fortes_e_boas(v: &Visao) -> (usize, usize) {
    let f = forcas(v);
    let fortes = f.iter().filter(|(_, x)| *x >= 8).count();
    let boas = f.iter().filter(|(_, x)| (6..8).contains(x)).count();
    (fortes, boas)
}

/// A decisão do bot, a partir só do que ele pode ver.
pub(crate) fn decidir(v: &Visao, rng: &mut impl Rng) -> Option<Acao> {
    let pode = |a: &str| v.acoes.contains(&a);
    let (fortes, boas) = fortes_e_boas(v);

    // Mão de onze: aceitar vale 3 e arrisca a partida; correr dá 1 ao adversário. Aceita
    // com mão que mata, corre com mão fraca.
    if pode("onze_aceitar") {
        let aceita = fortes >= 1 || boas >= 2;
        return Some(Acao::Onze { aceita });
    }

    // Respondendo a um pedido de truco.
    if pode("aceitar") {
        if fortes >= 2 && pode("aumentar") {
            return Some(Acao::Aumentar);
        }
        if fortes >= 1 || boas >= 2 {
            return Some(Acao::Aceitar);
        }
        return Some(Acao::Correr);
    }

    if !pode("jogar") {
        return None;
    }

    // Pedir truco: mão muito boa, e nem sempre — um bot que pede toda vez que pode fica
    // legível em três mãos.
    if pode("pedir") && fortes >= 2 && rng.random_range(0..100) < 60 {
        return Some(Acao::Pedir);
    }

    Some(qual_carta(v, pode("jogar_coberta")))
}

/// Escolhe a carta. Três situações, e a diferença entre elas é toda a jogada do truco:
/// cobrir a carta alheia pelo mais barato que vence, descartar quando não dá, e puxar com
/// a mais forte.
fn qual_carta(v: &Visao, pode_cobrir: bool) -> Acao {
    let f = forcas(v);
    let manilha = Numero::do_rotulo(v.manilha).unwrap_or(Numero::Quatro);

    let mais_fraca = || f.iter().min_by_key(|(_, x)| *x).map_or(0, |(i, _)| *i);
    let mais_forte = || f.iter().max_by_key(|(_, x)| *x).map_or(0, |(i, _)| *i);

    // O que já está na mesa, de quem é, e com que força. Carta de costas não conta.
    let na_mesa: Vec<(usize, u8)> = v
        .mesa
        .iter()
        .filter_map(|j| j.carta.map(|c: Carta| (j.assento, c.forca(manilha))))
        .collect();

    let Some(&(assento_topo, forca_topo)) = na_mesa.iter().max_by_key(|(_, x)| *x) else {
        // Puxando a rodada: vai com a mais forte. Levar a primeira rodada é o que dá a
        // vantagem de empate nas outras duas.
        return Acao::Jogar {
            indice: mais_forte(),
            coberta: false,
        };
    };

    // Se quem está ganhando a rodada é da minha dupla, não gasto carta: descarto.
    if equipe_de(assento_topo) == v.equipe && assento_topo != v.assento {
        return Acao::Jogar {
            indice: mais_fraca(),
            coberta: pode_cobrir,
        };
    }

    // Senão, o mais barato que vence. Se nada vence, descarto — de costas quando permitido,
    // que é o que esconde do adversário o quanto a minha mão era ruim.
    match f
        .iter()
        .filter(|(_, x)| *x > forca_topo)
        .min_by_key(|(_, x)| *x)
    {
        Some(&(i, _)) => Acao::Jogar {
            indice: i,
            coberta: false,
        },
        None => Acao::Jogar {
            indice: mais_fraca(),
            coberta: pode_cobrir,
        },
    }
}

#[cfg(test)]
mod teste {
    use super::decidir;
    use rand::SeedableRng;
    use truco_regras::visao::Visao;
    use truco_regras::{Modo, Partida};

    /// Toda ação que o bot devolve é **legal**: `aplicar` nunca a recusa.
    ///
    /// Isto é mais forte que olhar o log por "o bot tentou jogada ilegal", e não depende de
    /// ter um subscritor de log no teste. Duzentas partidas por modo, com todos os assentos
    /// jogados pelo bot: se a estratégia ignorar `acoes` em algum caminho — pedir truco fora
    /// da vez, esconder carta na primeira rodada, responder pedido que não existe — um
    /// `Err` aparece aqui.
    #[test]
    fn o_bot_nunca_devolve_jogada_ilegal_e_a_partida_sempre_termina() {
        for modo in [Modo::UmContraUm, Modo::DoisContraDois] {
            for semente in 0..200u64 {
                let mut rng = rand::rngs::StdRng::seed_from_u64(semente);
                let mut p = Partida::nova(modo, &mut rng);
                let mut passos = 0;

                while p.vencedora.is_none() {
                    passos += 1;
                    assert!(
                        passos < 5_000,
                        "modo {modo:?} semente {semente}: a partida não terminou — \
                         sinal de que em algum estado nenhum assento tem ação"
                    );

                    // Procura o assento que tem o que fazer. Em estado legal há sempre um.
                    let mut agiu = false;
                    for assento in 0..p.assentos() {
                        let v = Visao::para(&p, assento);
                        let Some(acao) = decidir(&v, &mut rng) else {
                            continue;
                        };
                        p.aplicar(assento, acao, &mut rng).unwrap_or_else(|e| {
                            panic!(
                                "modo {modo:?} semente {semente}: o bot do assento {assento} \
                                 tentou {acao:?} e a regra recusou: {e}. acoes={:?}",
                                v.acoes
                            )
                        });
                        agiu = true;
                        break;
                    }
                    assert!(
                        agiu,
                        "modo {modo:?} semente {semente}: nenhum assento tinha ação, \
                         e a partida não acabou — estado morto"
                    );
                }
                assert!(
                    p.placar.iter().any(|x| *x >= 12),
                    "modo {modo:?} semente {semente}: terminou sem ninguém chegar a 12"
                );
            }
        }
    }
}
