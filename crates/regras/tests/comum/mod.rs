//! Ajudas de teste. Nada de regra aqui.

use truco_regras::carta::{Carta, Naipe, Numero};

/// Carta por texto curto: `"3p"` = três de paus, `"Ao"` = ás de ouros, `"Qe"`, `"Jc"`.
/// Naipes: `o`uros, `e`spadas, `c`opas, `p`aus.
pub fn c(s: &str) -> Carta {
    let mut it = s.chars();
    let n = it.next().expect("número");
    let na = it.next().expect("naipe");
    let numero = match n {
        '4' => Numero::Quatro,
        '5' => Numero::Cinco,
        '6' => Numero::Seis,
        '7' => Numero::Sete,
        'Q' => Numero::Dama,
        'J' => Numero::Valete,
        'K' => Numero::Rei,
        'A' => Numero::As,
        '2' => Numero::Dois,
        '3' => Numero::Tres,
        _ => panic!("número inválido: {n}"),
    };
    let naipe = match na {
        'o' => Naipe::Ouros,
        'e' => Naipe::Espadas,
        'c' => Naipe::Copas,
        'p' => Naipe::Paus,
        _ => panic!("naipe inválido: {na}"),
    };
    Carta::nova(numero, naipe)
}

/// Baralho de teste a partir de uma lista de textos curtos.
pub fn baralho(spec: &[&str]) -> Vec<Carta> {
    spec.iter().map(|s| c(s)).collect()
}

/// RNG determinístico, para quando o teste não se importa com as cartas.
pub fn rng() -> rand::rngs::StdRng {
    use rand::SeedableRng;
    rand::rngs::StdRng::seed_from_u64(20261001)
}
