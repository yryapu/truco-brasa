//! Regras da carta: baralho, ordem de força, manilha, e a representação Unicode.
//! Etiquetas `R-nn` em <https://github.com/yryapu/poliorketikos-truco-brasa>.

mod comum;
use comum::c;
use truco_regras::carta::{COSTAS, Carta, Naipe, Numero};

#[test]
fn r01_baralho_tem_quarenta_cartas_distintas_sem_8_9_10() {
    let b = Carta::baralho();
    assert_eq!(b.len(), 40, "R-01: baralho sujo tem 40 cartas");

    let distintas: std::collections::HashSet<_> = b.iter().collect();
    assert_eq!(distintas.len(), 40, "R-01: sem carta repetida");

    // Os 8, 9 e 10 existem no bloco Unicode mas não no baralho do truco.
    for cp in [0x1F0A8u32, 0x1F0A9, 0x1F0AA, 0x1F0B8, 0x1F0C9, 0x1F0DA] {
        let ch = char::from_u32(cp).unwrap();
        assert!(Carta::do_unicode(ch).is_none(), "R-01: {ch} (8/9/10) não é carta do truco");
    }
}

#[test]
fn r02_ordem_de_forca_crescente() {
    // 4 < 5 < 6 < 7 < Q < J < K < A < 2 < 3, com uma manilha que não é nenhum deles.
    let manilha = Numero::Seis;
    let ordem = ["4p", "5p", "7p", "Qp", "Jp", "Kp", "Ap", "2p", "3p"];
    for par in ordem.windows(2) {
        let (a, b) = (c(par[0]), c(par[1]));
        assert!(
            a.forca(manilha) < b.forca(manilha),
            "R-02: {} deveria ser mais fraca que {}",
            a,
            b
        );
    }
}

#[test]
fn r02_a_dama_eh_mais_fraca_que_o_valete() {
    // A regra mais fácil de errar de cabeça, e a que todas as quatro fontes confirmam.
    let m = Numero::Seis;
    assert!(c("Qp").forca(m) < c("Jo").forca(m), "R-02: Q < J no truco");
    assert!(c("Jp").forca(m) < c("Ko").forca(m), "R-02: J < K");
    assert!(c("7p").forca(m) < c("Qo").forca(m), "R-02: 7 < Q");
}

#[test]
fn r03_comuns_de_mesmo_numero_empatam_qualquer_que_seja_o_naipe() {
    let m = Numero::Seis;
    for a in Naipe::TODOS {
        for b in Naipe::TODOS {
            assert_eq!(
                Carta::nova(Numero::Tres, a).forca(m),
                Carta::nova(Numero::Tres, b).forca(m),
                "R-03: naipe não desempata carta comum"
            );
        }
    }
}

#[test]
fn r04_a_manilha_eh_o_numero_seguinte_a_vira() {
    assert_eq!(Numero::Cinco.seguinte(), Numero::Seis, "R-04: vira 5 → manilha 6");
    assert_eq!(Numero::Valete.seguinte(), Numero::Rei, "R-04: vira J → manilha K (J<K<A)");
    assert_eq!(Numero::Dama.seguinte(), Numero::Valete, "R-04: vira Q → manilha J");
}

#[test]
fn r05_a_ordem_eh_circular_vira_tres_da_manilha_quatro() {
    assert_eq!(Numero::Tres.seguinte(), Numero::Quatro, "R-05: vira 3 → manilha 4");
}

#[test]
fn r06_manilha_bate_qualquer_comum_inclusive_o_tres() {
    let m = Numero::Quatro; // vira foi um 3
    assert!(
        c("4o").forca(m) > c("3p").forca(m),
        "R-06: a manilha mais fraca (4 de ouros) bate o 3 de paus"
    );
    for n in Numero::TODOS.into_iter().filter(|n| *n != m) {
        let comum = Carta::nova(n, Naipe::Paus);
        assert!(comum.forca(m) < c("4o").forca(m), "R-06: manilha bate {comum}");
    }
}

#[test]
fn r07_entre_manilhas_desempata_o_naipe_ouros_espadas_copas_paus() {
    let m = Numero::Seis;
    let escada = ["6o", "6e", "6c", "6p"];
    for par in escada.windows(2) {
        assert!(
            c(par[0]).forca(m) < c(par[1]).forca(m),
            "R-07: {} < {}",
            c(par[0]),
            c(par[1])
        );
    }
    assert_eq!(Naipe::Paus.apelido(), "zap");
    assert_eq!(Naipe::Ouros.apelido(), "picafumo");
}

#[test]
fn r08_o_naipe_da_vira_nao_importa() {
    for n in Naipe::TODOS {
        let vira = Carta::nova(Numero::Cinco, n);
        assert_eq!(vira.numero.seguinte(), Numero::Seis, "R-08: a vira define só o número");
    }
}

// --- §8 da especificação: a carta no protocolo é o caractere Unicode -------------------

#[test]
fn unicode_bate_com_o_exemplo_do_enunciado() {
    // "🂡 🂱 🃁 🃑" — os quatro ases, espadas, copas, ouros, paus.
    assert_eq!(c("Ae").unicode(), '🂡');
    assert_eq!(c("Ac").unicode(), '🂱');
    assert_eq!(c("Ao").unicode(), '🃁');
    assert_eq!(c("Ap").unicode(), '🃑');
}

#[test]
fn unicode_ida_e_volta_nas_quarenta() {
    for carta in Carta::baralho() {
        let ch = carta.unicode();
        assert_eq!(Carta::do_unicode(ch), Some(carta), "ida e volta de {carta}");
    }
}

#[test]
fn unicode_recusa_o_cavaleiro_que_o_baralho_frances_nao_tem() {
    // O bloco Unicode tem KNIGHT em 0x_C, entre o Valete e a Dama. Indexar as figuras em
    // sequência colocaria a Dama no Cavaleiro — foi o erro que este teste impede.
    for ch in ['🂬', '🂼', '🃌', '🃜'] {
        assert!(Carta::do_unicode(ch).is_none(), "{ch} é KNIGHT, não existe no truco");
    }
    assert_eq!(c("Qp").unicode(), '🃝', "a Dama de paus é 0x1F0DD, não 0x1F0DC");
    assert_eq!(c("Kp").unicode(), '🃞');
    assert_eq!(c("Jp").unicode(), '🃛');
}

#[test]
fn costas_eh_o_playing_card_back() {
    assert_eq!(COSTAS, '🂠');
    assert!(Carta::do_unicode(COSTAS).is_none(), "as costas não são uma carta jogável");
}

#[test]
fn no_json_a_carta_eh_o_caractere_e_nada_mais() {
    let j = serde_json::to_string(&c("Ap")).unwrap();
    assert_eq!(j, "\"🃑\"", "o protocolo manda o caractere, não um objeto inventado");
    let volta: Carta = serde_json::from_str("\"🃑\"").unwrap();
    assert_eq!(volta, c("Ap"));
    assert!(serde_json::from_str::<Carta>("\"As de paus\"").is_err());
    assert!(serde_json::from_str::<Carta>("\"🂬\"").is_err(), "KNIGHT não entra");
}
