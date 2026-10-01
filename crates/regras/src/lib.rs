//! Regras do truco paulista — puras, sem I/O.
//!
//! Cada regra implementada aqui cita a sua etiqueta `R-nn`, que está em
//! `regras/truco-paulista.md` do repositório de pesquisa
//! (<https://github.com/yryapu/poliorketikos-truco-brasa>) junto da fonte que a sustenta.
//! Regra sem etiqueta é decisão nossa, e está marcada `[DECISÃO]` lá.

pub mod carta;
pub mod partida;
pub mod visao;

pub use carta::{COSTAS, Carta, Naipe, Numero};
pub use partida::{Acao, Aviso, Erro, Modo, Partida, TipoMao, equipe_de};
