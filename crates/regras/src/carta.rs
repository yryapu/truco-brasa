//! A carta do truco paulista, e a sua representação no protocolo.
//!
//! O protocolo exige o **próprio caractere Unicode** da carta (bloco *Playing Cards*,
//! `U+1F0A0`–`U+1F0FF`). Ver `regras/truco-paulista.md` §8 no repositório de pesquisa.

use serde::{Deserialize, Serialize, Serializer, de::Error as _};
use std::fmt;

/// Naipe. A ordem da enumeração **é** a ordem de força entre manilhas (R-07):
/// `ouros < espadas < copas < paus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Naipe {
    Ouros,
    Espadas,
    Copas,
    Paus,
}

impl Naipe {
    pub const TODOS: [Naipe; 4] = [Naipe::Ouros, Naipe::Espadas, Naipe::Copas, Naipe::Paus];

    /// Base do naipe no bloco Unicode.
    const fn base_unicode(self) -> u32 {
        match self {
            Naipe::Espadas => 0x1F0A0,
            Naipe::Copas => 0x1F0B0,
            Naipe::Ouros => 0x1F0C0,
            Naipe::Paus => 0x1F0D0,
        }
    }

    /// O apelido de mesa da manilha deste naipe (fonte F2).
    pub const fn apelido(self) -> &'static str {
        match self {
            Naipe::Paus => "zap",
            Naipe::Copas => "copeta",
            Naipe::Espadas => "espadilha",
            Naipe::Ouros => "picafumo",
        }
    }
}

/// Número da carta. A ordem da enumeração **é** a ordem de força (R-02):
/// `4 < 5 < 6 < 7 < Q < J < K < A < 2 < 3`.
///
/// Repare em `Dama` **antes** de `Valete`: a Dama é mais fraca que o Valete no truco, o que
/// contraria a intuição de quem vem de outros jogos. É a regra mais fácil de errar de
/// cabeça, e por isso tem teste próprio (ver `tests::r02_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Numero {
    Quatro,
    Cinco,
    Seis,
    Sete,
    Dama,
    Valete,
    Rei,
    As,
    Dois,
    Tres,
}

impl Numero {
    pub const TODOS: [Numero; 10] = [
        Numero::Quatro,
        Numero::Cinco,
        Numero::Seis,
        Numero::Sete,
        Numero::Dama,
        Numero::Valete,
        Numero::Rei,
        Numero::As,
        Numero::Dois,
        Numero::Tres,
    ];

    /// Deslocamento no bloco Unicode.
    ///
    /// **Cuidado:** o bloco tem uma figura que o baralho francês não tem — o `KNIGHT` em
    /// `0x_C`. Por isso a Dama é `0xD` e o Rei `0xE`; indexar as figuras em sequência
    /// (`J=0xB, Q=0xC, K=0xD`) colocaria a Dama no Cavaleiro e deslocaria o Rei.
    const fn deslocamento_unicode(self) -> u32 {
        match self {
            Numero::As => 0x1,
            Numero::Dois => 0x2,
            Numero::Tres => 0x3,
            Numero::Quatro => 0x4,
            Numero::Cinco => 0x5,
            Numero::Seis => 0x6,
            Numero::Sete => 0x7,
            Numero::Valete => 0xB,
            Numero::Dama => 0xD,
            Numero::Rei => 0xE,
        }
    }

    /// O número seguinte na ordem de força, de forma **circular** — é o que faz a vira `3`
    /// dar manilha `4` (R-05).
    pub fn seguinte(self) -> Numero {
        let i = Numero::TODOS
            .iter()
            .position(|n| *n == self)
            .expect("número está em TODOS");
        Numero::TODOS[(i + 1) % Numero::TODOS.len()]
    }
}

/// Uma carta do baralho de 40 (R-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Carta {
    pub numero: Numero,
    pub naipe: Naipe,
}

/// O caractere `PLAYING CARD BACK`, usado pela carta jogada de costas (R-14).
pub const COSTAS: char = '\u{1F0A0}';

impl Carta {
    pub const fn nova(numero: Numero, naipe: Naipe) -> Self {
        Self { numero, naipe }
    }

    /// O caractere Unicode desta carta — o que trafega no protocolo.
    pub fn unicode(self) -> char {
        let cp = self.naipe.base_unicode() + self.numero.deslocamento_unicode();
        char::from_u32(cp).expect("todo ponto de código do baralho de 40 é um caractere válido")
    }

    /// A volta: do caractere para a carta. Recusa o que não é carta do truco — incluindo os
    /// `8`, `9`, `10` e os quatro `KNIGHT`, que existem no bloco Unicode mas não no baralho.
    pub fn do_unicode(c: char) -> Option<Self> {
        let cp = c as u32;
        let naipe = match cp & !0xF {
            0x1F0A0 => Naipe::Espadas,
            0x1F0B0 => Naipe::Copas,
            0x1F0C0 => Naipe::Ouros,
            0x1F0D0 => Naipe::Paus,
            _ => return None,
        };
        let numero = Numero::TODOS
            .into_iter()
            .find(|n| n.deslocamento_unicode() == (cp & 0xF))?;
        Some(Carta::nova(numero, naipe))
    }

    /// O baralho sujo completo: 40 cartas (R-01).
    pub fn baralho() -> Vec<Carta> {
        Numero::TODOS
            .into_iter()
            .flat_map(|n| Naipe::TODOS.into_iter().map(move |s| Carta::nova(n, s)))
            .collect()
    }

    /// Força desta carta nesta mão, dada a manilha (R-02, R-06, R-07).
    ///
    /// Comuns ficam em `0..=9` pela ordem de `Numero`; manilhas em `10..=13` pela ordem de
    /// `Naipe`. Logo qualquer manilha bate qualquer comum, inclusive o `3`, e duas comuns de
    /// mesmo número têm a **mesma** força — o que é o empate de R-03.
    pub fn forca(self, manilha: Numero) -> u8 {
        if self.numero == manilha {
            10 + self.naipe as u8
        } else {
            self.numero as u8
        }
    }

    pub fn eh_manilha(self, manilha: Numero) -> bool {
        self.numero == manilha
    }
}

impl fmt::Display for Carta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.unicode())
    }
}

/// No protocolo a carta é **o caractere**, não um objeto. `{"carta":"🂡"}`, nunca
/// `{"carta":{"numero":"As","naipe":"Espadas"}}`.
impl Serialize for Carta {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.unicode().encode_utf8(&mut [0u8; 4]))
    }
}

impl<'de> Deserialize<'de> for Carta {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let mut cs = s.chars();
        let (Some(c), None) = (cs.next(), cs.next()) else {
            return Err(D::Error::custom(format!(
                "esperava um caractere de carta, veio {s:?}"
            )));
        };
        Carta::do_unicode(c).ok_or_else(|| D::Error::custom(format!("{c:?} não é carta do truco")))
    }
}
