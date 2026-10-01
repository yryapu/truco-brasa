//! Regras da mão e da partida. Uma função por regra, com a etiqueta no nome.

mod comum;
use comum::{baralho, c, rng};
use truco_regras::partida::{
    ALVO, Acao, Erro, FimDaMao, Modo, Partida, Rodada, TipoMao, decidir_mao, equipe_de,
    proximo_valor, resolver_rodada,
};

/// Atalho: aplica e exige sucesso.
fn ok(p: &mut Partida, assento: usize, a: Acao) {
    let mut r = rng();
    p.aplicar(assento, a, &mut r)
        .unwrap_or_else(|e| panic!("assento {assento} {a:?}: {e}"));
}
/// Atalho: aplica e exige este erro.
fn nega(p: &mut Partida, assento: usize, a: Acao, esperado: Erro) {
    let mut r = rng();
    let got = p.aplicar(assento, a, &mut r).expect_err("deveria recusar");
    assert_eq!(got, esperado, "assento {assento} {a:?}");
}
fn joga(p: &mut Partida, assento: usize, indice: usize) {
    ok(
        p,
        assento,
        Acao::Jogar {
            indice,
            coberta: false,
        },
    )
}
fn esconde(p: &mut Partida, assento: usize, indice: usize) {
    ok(
        p,
        assento,
        Acao::Jogar {
            indice,
            coberta: true,
        },
    )
}

// ---------------------------------------------------------------- R-09, R-13, R-27

#[test]
fn r09_tres_cartas_por_jogador_e_uma_vira() {
    for modo in [Modo::UmContraUm, Modo::DoisContraDois] {
        let p = Partida::nova(modo, &mut rng());
        assert_eq!(p.mao.cartas.len(), modo.assentos());
        for mao in &p.mao.cartas {
            assert_eq!(mao.len(), 3, "R-09: três cartas");
        }
        // A vira não está na mão de ninguém.
        for mao in &p.mao.cartas {
            assert!(
                !mao.contains(&p.mao.vira),
                "R-09: a vira sai do monte, não da mão"
            );
        }
    }
}

#[test]
fn r13_o_direito_de_puxar_anda_um_assento_por_mao() {
    // 1x1, mão 0 puxada pelo assento 0; quem corre de um truco encerra a mão na hora.
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "4e", "5e", "3c", "4c", "5c", "5p"]),
    );
    assert_eq!(p.mao.puxador, 0);
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Correr);
    assert_eq!(p.numero_da_mao, 1);
    assert_eq!(p.mao.puxador, 1, "R-13: a mão 1 é puxada pelo assento 1");
    assert_eq!(p.mao.vez, 1);
}

#[test]
fn r27_chega_ou_passa_de_doze_e_a_partida_acaba() {
    // 9x9, uma mão trucada valendo 3: 12x9 e fim, sem passar pela mão de onze.
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [9, 9],
        &baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]),
    );
    assert!(
        matches!(p.mao.tipo, TipoMao::Normal),
        "9x9 não é mão de onze"
    );
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Aceitar);
    assert_eq!(p.mao.valor, 3);
    joga(&mut p, 0, 0); // 3e
    joga(&mut p, 1, 0); // 4c — perde
    joga(&mut p, 0, 0); // 4e
    joga(&mut p, 1, 0); // 5c — perde (manilha é 2, vira Ap)
    assert_eq!(p.placar, [12, 9]);
    assert_eq!(p.vencedora, Some(0), "R-27: 12 encerra");
    assert!(p.placar[0] >= ALVO);
    nega(&mut p, 0, Acao::Pedir, Erro::PartidaTerminada);
}

// ---------------------------------------------------------------- R-10, R-11 (empates)

#[test]
fn r11_tabela_de_empate_caso_a_caso() {
    let r = |v: Option<u8>| Rodada {
        vencedor: v,
        puxador_seguinte: 0,
    };

    // Uma rodada nunca decide a mão.
    assert_eq!(
        decidir_mao(&[r(Some(0))]),
        None,
        "R-10: uma rodada não fecha a mão"
    );
    assert_eq!(decidir_mao(&[r(None)]), None);

    // "vencer uma e empatar outra" (R-10), nas duas ordens.
    assert_eq!(
        decidir_mao(&[r(None), r(Some(1))]),
        Some(Some(1)),
        "empate na 1ª, vence a 2ª"
    );
    assert_eq!(
        decidir_mao(&[r(Some(0)), r(None)]),
        Some(Some(0)),
        "empate na 2ª, vence a 1ª"
    );

    // Duas rodadas para a mesma dupla.
    assert_eq!(decidir_mao(&[r(Some(1)), r(Some(1))]), Some(Some(1)));

    // Uma para cada: vai para a terceira.
    assert_eq!(decidir_mao(&[r(Some(0)), r(Some(1))]), None);
    assert_eq!(
        decidir_mao(&[r(Some(0)), r(Some(1)), r(Some(1))]),
        Some(Some(1))
    );

    // Empate na 3ª com a 1ª decidida: leva quem venceu a 1ª.
    assert_eq!(
        decidir_mao(&[r(Some(0)), r(Some(1)), r(None)]),
        Some(Some(0)),
        "R-11: empate na 3ª, ganha quem fez a 1ª"
    );

    // 1ª e 2ª empatadas: a 3ª decide.
    assert_eq!(decidir_mao(&[r(None), r(None)]), None);
    assert_eq!(decidir_mao(&[r(None), r(None), r(Some(0))]), Some(Some(0)));

    // As três empatadas: ninguém pontua.
    assert_eq!(
        decidir_mao(&[r(None), r(None), r(None)]),
        Some(None),
        "R-11: três empates, ninguém ganha ponto"
    );
}

#[test]
fn r11_tres_empates_na_mesa_nao_dao_ponto_a_ninguem() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "4e", "5e", "3c", "4c", "5c", "5p"]),
    );
    for _ in 0..3 {
        joga(&mut p, 0, 0);
        joga(&mut p, 1, 0);
    }
    assert_eq!(p.placar, [0, 0], "R-11: ninguém pontua");
    assert_eq!(p.numero_da_mao, 1, "a mão seguinte começou");
}

#[test]
fn r12_quem_puxa_a_rodada_seguinte() {
    use truco_regras::Numero;
    use truco_regras::partida::Jogada;
    let jog = |assento, carta, coberta| Jogada {
        assento,
        carta: c(carta),
        coberta,
    };
    let manilha = Numero::Seis; // vira foi um 5

    // Vencedor da rodada puxa a seguinte.
    let r = resolver_rodada(&[jog(0, "4e", false), jog(1, "3c", false)], manilha);
    assert_eq!(r.vencedor, Some(1));
    assert_eq!(r.puxador_seguinte, 1, "R-12: quem venceu puxa");

    // Empate: puxa quem pôs a PRIMEIRA carta do empate.
    let r = resolver_rodada(
        &[
            jog(0, "7e", false),
            jog(1, "3c", false),
            jog(2, "3p", false),
        ],
        manilha,
    );
    assert_eq!(
        r.vencedor, None,
        "R-03: dois 3 empatam, de duplas diferentes"
    );
    assert_eq!(
        r.puxador_seguinte, 1,
        "R-12: a primeira carta que empatou foi do assento 1"
    );

    // Dois parceiros no topo não é empate: a dupla levou a rodada.
    let r = resolver_rodada(
        &[
            jog(0, "3e", false),
            jog(1, "4c", false),
            jog(2, "3p", false),
        ],
        manilha,
    );
    assert_eq!(
        r.vencedor,
        Some(0),
        "parceiros empatados no topo: a dupla ganha"
    );
    assert_eq!(r.puxador_seguinte, 0);

    // Manilhas nunca empatam (R-07).
    let r = resolver_rodada(&[jog(0, "6o", false), jog(1, "6p", false)], manilha);
    assert_eq!(r.vencedor, Some(1), "R-07: zap bate picafumo");
}

// ---------------------------------------------------------------- R-14 (carta de costas)

#[test]
fn r14_nao_se_esconde_carta_na_primeira_rodada() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "3c", "4e", "4c", "5c", "7c", "Ap"]),
    );
    nega(
        &mut p,
        0,
        Acao::Jogar {
            indice: 0,
            coberta: true,
        },
        Erro::CobertaNaPrimeiraRodada,
    );
}

#[test]
fn r14_carta_de_costas_nao_disputa_a_rodada() {
    // vira Ap ⇒ manilha 2. Assento 0 esconde um 3 e perde a rodada para um 5.
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "3c", "4e", "4c", "5c", "7c", "Ap"]),
    );
    joga(&mut p, 0, 0); // 3e
    joga(&mut p, 1, 0); // 4c
    assert_eq!(p.mao.rodadas[0].vencedor, Some(0));

    esconde(&mut p, 0, 0); // 3c, de costas — a mais forte da mesa, e não vale nada
    joga(&mut p, 1, 0); // 5c
    assert_eq!(
        p.mao.rodadas[1].vencedor,
        Some(1),
        "R-14: a carta de costas é desconsiderada, então o 5 ganha do 3 escondido"
    );
}

#[test]
fn r14_todas_de_costas_empata_a_rodada() {
    use truco_regras::Aviso;
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "3c", "4e", "4c", "5c", "7c", "Ap"]),
    );
    joga(&mut p, 0, 0); // 3e
    joga(&mut p, 1, 0); // 4c — o assento 0 leva a primeira
    esconde(&mut p, 0, 0);
    let avisos = p
        .aplicar(
            1,
            Acao::Jogar {
                indice: 0,
                coberta: true,
            },
            &mut rng(),
        )
        .expect("as duas de costas na segunda rodada");

    let resolvida = avisos
        .iter()
        .find_map(|a| match a {
            Aviso::RodadaResolvida { vencedora, .. } => Some(*vencedora),
            _ => None,
        })
        .expect("a rodada foi resolvida");
    assert_eq!(
        resolvida, None,
        "[DECISÃO] ninguém disputou: a rodada empata"
    );

    // E o empate fecha a mão para quem venceu a primeira (R-10), então o placar anda — é
    // por isso que este teste lê o aviso e não `p.mao.rodadas`: a mão já é outra.
    assert_eq!(p.placar, [1, 0], "R-10: venceu uma e empatou outra");
}

// ---------------------------------------------------------------- R-15..R-21 (truco)

#[test]
fn r15_a_escada_eh_1_3_6_9_12_e_nao_tem_outro_valor() {
    assert_eq!(proximo_valor(1), Some(3));
    assert_eq!(proximo_valor(3), Some(6));
    assert_eq!(proximo_valor(6), Some(9));
    assert_eq!(proximo_valor(9), Some(12));
    assert_eq!(proximo_valor(12), None, "R-15: 12 é o teto");
    assert_eq!(proximo_valor(2), None, "R-15: não existe mão valendo 2");
}

#[test]
fn r15_r20_a_escada_inteira_e_o_teto_no_doze() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]),
    );
    ok(&mut p, 0, Acao::Pedir); // truco → 3
    assert_eq!(p.mao.pendencia.unwrap().valor_proposto, 3);
    ok(&mut p, 1, Acao::Aumentar); // seis
    assert_eq!(
        (p.mao.valor, p.mao.pendencia.unwrap().valor_proposto),
        (3, 6)
    );
    ok(&mut p, 0, Acao::Aumentar); // nove
    assert_eq!(
        (p.mao.valor, p.mao.pendencia.unwrap().valor_proposto),
        (6, 9)
    );
    ok(&mut p, 1, Acao::Aumentar); // doze
    assert_eq!(
        (p.mao.valor, p.mao.pendencia.unwrap().valor_proposto),
        (9, 12)
    );
    nega(&mut p, 0, Acao::Aumentar, Erro::NoDozeNaoSeAumenta);
    ok(&mut p, 0, Acao::Aceitar);
    assert_eq!(p.mao.valor, 12);
    // A vez volta para quem propôs o doze (assento 1), e não há degrau acima.
    assert_eq!(p.mao.vez, 1);
    nega(&mut p, 1, Acao::Pedir, Erro::ValorNoTeto);
}

#[test]
fn r16_so_se_pede_na_propria_vez() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]),
    );
    assert_eq!(p.mao.vez, 0);
    nega(&mut p, 1, Acao::Pedir, Erro::NaoEhSuaVez);
    nega(
        &mut p,
        1,
        Acao::Jogar {
            indice: 0,
            coberta: false,
        },
        Erro::NaoEhSuaVez,
    );
}

#[test]
fn r17_ninguem_joga_carta_com_pedido_na_mesa() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]),
    );
    ok(&mut p, 0, Acao::Pedir);
    nega(
        &mut p,
        1,
        Acao::Jogar {
            indice: 0,
            coberta: false,
        },
        Erro::RespondaOPedido,
    );
    nega(
        &mut p,
        0,
        Acao::Jogar {
            indice: 0,
            coberta: false,
        },
        Erro::RespondaOPedido,
    );
}

#[test]
fn r18_quem_corre_entrega_o_valor_anterior_ao_pedido() {
    // Correr de um truco: 1 ponto.
    let deck = baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]);
    let mut p = Partida::com_baralho(Modo::UmContraUm, [0, 0], &deck);
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Correr);
    assert_eq!(p.placar, [1, 0], "R-18: correr de um truco dá 1");

    // Correr de um seis: 3 pontos para quem pediu o seis.
    let mut p = Partida::com_baralho(Modo::UmContraUm, [0, 0], &deck);
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Aumentar);
    ok(&mut p, 0, Acao::Correr);
    assert_eq!(p.placar, [0, 3], "R-18: correr de um seis dá 3");

    // Correr de um nove: 6. Correr de um doze: 9.
    let mut p = Partida::com_baralho(Modo::UmContraUm, [0, 0], &deck);
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Aumentar);
    ok(&mut p, 0, Acao::Aumentar);
    ok(&mut p, 1, Acao::Correr);
    assert_eq!(p.placar, [6, 0], "R-18: correr de um nove dá 6");

    let mut p = Partida::com_baralho(Modo::UmContraUm, [0, 0], &deck);
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Aumentar);
    ok(&mut p, 0, Acao::Aumentar);
    ok(&mut p, 1, Acao::Aumentar);
    ok(&mut p, 0, Acao::Correr);
    assert_eq!(p.placar, [0, 9], "R-18: correr de um doze dá 9");
}

#[test]
fn r19_a_mesma_dupla_nao_pede_duas_vezes_seguidas() {
    // 2x2: duplas {0,2} e {1,3}.
    let mut p = Partida::com_baralho(
        Modo::DoisContraDois,
        [0, 0],
        &baralho(&[
            "4e", "5e", "7e", "4c", "5c", "7c", "4o", "5o", "7o", "4p", "5p", "7p", "Ae",
        ]),
    );
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Aceitar);
    assert_eq!(
        (p.mao.valor, p.mao.vez),
        (3, 0),
        "a vez volta para quem pediu"
    );
    joga(&mut p, 0, 0);
    joga(&mut p, 1, 0);
    assert_eq!(p.mao.vez, 2);
    nega(&mut p, 2, Acao::Pedir, Erro::PedidoSeguidoDaMesmaDupla);
    joga(&mut p, 2, 0);
    // Agora o assento 3, da outra dupla, pode pedir.
    ok(&mut p, 3, Acao::Pedir);
    assert_eq!(p.mao.pendencia.unwrap().valor_proposto, 6);
}

#[test]
fn r21_o_valor_nao_atravessa_a_mao() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]),
    );
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Aceitar);
    assert_eq!(p.mao.valor, 3);
    joga(&mut p, 0, 0);
    joga(&mut p, 1, 0);
    joga(&mut p, 0, 0);
    joga(&mut p, 1, 0);
    assert_eq!(p.placar, [3, 0]);
    assert_eq!(p.mao.valor, 1, "R-21: a mão seguinte começa valendo 1");
    assert_eq!(
        p.mao.ultima_equipe_pedinte, None,
        "e qualquer dupla pode pedir o primeiro"
    );
}

// ---------------------------------------------------------------- R-22..R-26 (onze, ferro)

#[test]
fn r22_r23_mao_de_onze_recusada_da_um_ponto_ao_adversario() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [11, 0],
        &baralho(&["4e", "5e", "7e", "4c", "5c", "7c", "Ae"]),
    );
    assert_eq!(p.mao.tipo, TipoMao::Onze { equipe: 0 });
    assert!(p.mao.aguarda_onze());
    assert_eq!(p.mao.valor, 3, "R-23: a mão de onze já começa valendo 3");

    // R-22: nada acontece antes da decisão.
    nega(
        &mut p,
        0,
        Acao::Jogar {
            indice: 0,
            coberta: false,
        },
        Erro::DecidaAMaoDeOnze,
    );
    nega(&mut p, 0, Acao::Pedir, Erro::DecidaAMaoDeOnze);
    // Só a dupla que está com 11 decide.
    nega(
        &mut p,
        1,
        Acao::Onze { aceita: true },
        Erro::NaoEhSuaMaoDeOnze,
    );

    ok(&mut p, 0, Acao::Onze { aceita: false });
    assert_eq!(p.placar, [11, 1], "R-23: correu, o adversário leva 1");
    assert!(p.mao.fim.is_none(), "a mão nova já começou");
    assert_eq!(p.numero_da_mao, 1);
}

#[test]
fn r24_na_mao_de_onze_nao_se_pede_seis() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [11, 0],
        &baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]),
    );
    ok(&mut p, 0, Acao::Onze { aceita: true });
    assert!(!p.mao.aguarda_onze());
    assert_eq!(p.mao.valor, 3);
    nega(&mut p, 0, Acao::Pedir, Erro::PedidoProibidoNestaMao);

    // Aceitou e venceu: 11 + 3 = 14, partida ganha.
    joga(&mut p, 0, 0);
    joga(&mut p, 1, 0);
    joga(&mut p, 0, 0);
    joga(&mut p, 1, 0);
    assert_eq!(p.placar, [14, 0]);
    assert_eq!(p.vencedora, Some(0));
}

#[test]
fn r25_na_mao_de_onze_a_dupla_ve_as_cartas_entre_si() {
    use truco_regras::visao::Visao;
    let mut p = Partida::com_baralho(
        Modo::DoisContraDois,
        [11, 0],
        &baralho(&[
            "4e", "5e", "7e", "4c", "5c", "7c", "4o", "5o", "7o", "4p", "5p", "7p", "Ae",
        ]),
    );
    let v0 = Visao::para(&p, 0);
    assert_eq!(
        v0.cartas_do_parceiro.as_deref(),
        Some(&p.mao.cartas[2][..]),
        "R-25: na mão de onze o parceiro é visível"
    );
    // O adversário não ganha nada com isso.
    assert!(Visao::para(&p, 1).cartas_do_parceiro.is_none());

    // Fora da mão de onze, ninguém vê ninguém. E repare que não dá para testar isso
    // recusando a mão de onze: por F2, uma vez em 11 pontos, **toda** mão seguinte é mão de
    // onze — "a partir deste momento, no começo de cada mão esta dupla pode escolher".
    ok(&mut p, 0, Acao::Onze { aceita: false });
    assert_eq!(
        p.mao.tipo,
        TipoMao::Onze { equipe: 0 },
        "ainda em 11: a mão seguinte também é"
    );

    let normal = Partida::com_baralho(
        Modo::DoisContraDois,
        [0, 0],
        &baralho(&[
            "4e", "5e", "7e", "4c", "5c", "7c", "4o", "5o", "7o", "4p", "5p", "7p", "Ae",
        ]),
    );
    assert!(
        Visao::para(&normal, 0).cartas_do_parceiro.is_none(),
        "R-25: só nesse momento do jogo"
    );
}

#[test]
fn r26_mao_de_ferro_vale_um_sem_truco_e_decide_a_partida() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [11, 11],
        &baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]),
    );
    assert_eq!(p.mao.tipo, TipoMao::Ferro);
    assert_eq!(p.mao.valor, 1, "R-26: a mão de ferro vale 1");
    assert!(!p.mao.aguarda_onze(), "R-26: ninguém decide nada");
    nega(&mut p, 0, Acao::Onze { aceita: true }, Erro::NaoEhMaoDeOnze);
    nega(&mut p, 0, Acao::Pedir, Erro::PedidoProibidoNestaMao);

    joga(&mut p, 0, 0);
    joga(&mut p, 1, 0);
    joga(&mut p, 0, 0);
    joga(&mut p, 1, 0);
    assert_eq!(p.placar, [12, 11]);
    assert_eq!(
        p.vencedora,
        Some(0),
        "R-26: quem vence a mão de ferro vence a partida"
    );
}

// ---------------------------------------------------------------- R-28..R-31 (1x1)

#[test]
fn r28_r29_o_1x1_usa_o_mesmo_baralho_e_cada_um_eh_a_sua_dupla() {
    let p = Partida::nova(Modo::UmContraUm, &mut rng());
    assert_eq!(p.assentos(), 2);
    assert_eq!(equipe_de(0), 0);
    assert_eq!(equipe_de(1), 1);
    assert_eq!(p.mao.cartas.iter().map(Vec::len).sum::<usize>(), 6);
}

#[test]
fn r30_mao_de_onze_em_1x1_o_jogador_ve_as_proprias_cartas_e_decide() {
    use truco_regras::visao::Visao;
    let p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 11],
        &baralho(&["4e", "5e", "7e", "4c", "5c", "7c", "Ae"]),
    );
    assert_eq!(p.mao.tipo, TipoMao::Onze { equipe: 1 });
    let v = Visao::para(&p, 1);
    assert_eq!(v.minhas_cartas.len(), 3, "R-30: decide sabendo a mão");
    assert!(
        v.cartas_do_parceiro.is_none(),
        "R-30: não há parceiro em 1x1"
    );
    assert!(v.acoes.contains(&"onze_aceitar") && v.acoes.contains(&"onze_correr"));
    assert!(
        Visao::para(&p, 0).acoes.is_empty(),
        "o adversário não decide nem joga agora"
    );
}

#[test]
fn r31_em_1x1_os_pedidos_alternam_naturalmente() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]),
    );
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Aceitar);
    joga(&mut p, 0, 0);
    // Agora é a vez do 1, que não fez o último pedido? Fez: ele aceitou, não pediu.
    // Quem pediu foi a dupla 0, então a dupla 1 pode pedir o seis.
    ok(&mut p, 1, Acao::Pedir);
    assert_eq!(p.mao.pendencia.unwrap().valor_proposto, 6);
    nega(&mut p, 1, Acao::Pedir, Erro::RespondaOPedido);
}

// ---------------------------------------------------------------- a visão não vaza

#[test]
fn visao_nunca_vaza_carta_de_outro_assento() {
    use truco_regras::visao::Visao;
    let mut p = Partida::nova(Modo::DoisContraDois, &mut rng());
    // Uma rodada inteira, com uma carta de costas na segunda.
    let n = p.assentos();
    for _ in 0..n {
        let vez = p.mao.vez;
        joga(&mut p, vez, 0);
    }
    let vez = p.mao.vez;
    esconde(&mut p, vez, 0);

    for assento in 0..n {
        let json = serde_json::to_string(&Visao::para(&p, assento)).unwrap();
        for outro in 0..n {
            if outro == assento {
                continue;
            }
            // Na mão de onze o parceiro é visível por regra; esta partida está 0x0.
            for carta in &p.mao.cartas[outro] {
                assert!(
                    !json.contains(carta.unicode()),
                    "assento {assento} viu {carta}, que é do assento {outro}"
                );
            }
        }
        // A carta jogada de costas não aparece para ninguém, nem para quem a jogou na mesa.
        let v = Visao::para(&p, assento);
        let coberta = v
            .mesa
            .iter()
            .find(|j| j.coberta)
            .expect("há uma carta de costas");
        assert!(
            coberta.carta.is_none(),
            "R-14: a carta de costas não vai no estado"
        );
    }
}

#[test]
fn fim_da_mao_carrega_os_pontos_que_foram_para_o_placar() {
    let mut p = Partida::com_baralho(
        Modo::UmContraUm,
        [0, 0],
        &baralho(&["3e", "2e", "Ae", "4c", "5c", "6c", "7p"]),
    );
    ok(&mut p, 0, Acao::Pedir);
    ok(&mut p, 1, Acao::Correr);
    // A mão terminada é a anterior; o fim vem no aviso. Aqui conferimos o efeito no placar.
    assert_eq!(p.placar, [1, 0]);
    let fim = FimDaMao::Correu {
        vencedora: 0,
        pontos: 1,
    };
    assert_eq!(fim.vencedora(), Some(0));
    assert_eq!(fim.pontos(), 1);
}

#[test]
fn a_visao_so_contem_as_cartas_que_aquele_assento_tem_direito_de_ver() {
    use std::collections::BTreeSet;
    use truco_regras::visao::Visao;

    // Este teste é mais forte que o anterior de propósito. O outro perguntava "a visão
    // contém carta de outro assento?", e isso deixava passar um glifo de carta que não
    // viesse da mão de ninguém — foi exatamente o furo do indicador de manilha, que
    // desenhava a carta de paus daquele número e só colidia quando alguém tinha justamente
    // aquela carta. Aqui a pergunta é a outra: **todo** caractere de carta na visão tem de
    // estar no conjunto permitido.
    let mut p = Partida::nova(Modo::DoisContraDois, &mut rng());
    let n = p.assentos();

    // Uma rodada completa e meia, com uma carta de costas.
    for _ in 0..n {
        let vez = p.mao.vez;
        joga(&mut p, vez, 0);
    }
    let vez = p.mao.vez;
    esconde(&mut p, vez, 0);
    let vez = p.mao.vez;
    joga(&mut p, vez, 0);

    for assento in 0..n {
        let v = Visao::para(&p, assento);
        let json = serde_json::to_string(&v).unwrap();

        // Tudo que é carta, em qualquer campo, em qualquer nível.
        let encontradas: BTreeSet<char> = json
            .chars()
            .filter(|c| truco_regras::Carta::do_unicode(*c).is_some())
            .collect();

        let mut permitidas: BTreeSet<char> =
            p.mao.cartas[assento].iter().map(|c| c.unicode()).collect();
        permitidas.insert(p.mao.vira.unicode());
        for j in &p.mao.mesa {
            // Carta de costas não entra: ela não é visível para ninguém.
            if !j.coberta {
                permitidas.insert(j.carta.unicode());
            }
        }

        let sobrando: Vec<char> = encontradas.difference(&permitidas).copied().collect();
        assert!(
            sobrando.is_empty(),
            "assento {assento} recebeu {sobrando:?}, que não é nem a mão dele, nem a vira, \
             nem carta aberta na mesa"
        );
    }
}
