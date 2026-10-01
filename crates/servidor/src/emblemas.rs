//! Emblemas de reputação. **Derivados**, nunca guardados.
//!
//! A decisão de desenho: emblema é uma *função* da estatística do jogador, calculada na
//! leitura. Guardar emblema numa tabela criaria um segundo lugar onde a verdade mora — e
//! o dia em que o critério mudar, a tabela fica mentindo até alguém reprocessar. Assim,
//! mudar o critério é mudar esta função.
//!
//! O que o enunciado pede é "emblemas de reputação por vitórias e por histórico". Então há
//! dois eixos: quantidade de vitórias, e o que o histórico conta além da contagem —
//! sequência, volume de partidas, e lavada.

use crate::bd::Jogador;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Emblema {
    pub chave: &'static str,
    pub nome: &'static str,
    pub descricao: &'static str,
    /// Um caractere para a interface. Não é parte do contrato de dados.
    pub icone: &'static str,
}

const TODOS: &[(Emblema, fn(&Jogador) -> bool)] = &[
    (
        Emblema {
            chave: "estreante",
            nome: "Estreante",
            descricao: "ainda não terminou uma partida",
            icone: "🌱",
        },
        |j| j.partidas == 0,
    ),
    (
        Emblema {
            chave: "batismo",
            nome: "Batismo",
            descricao: "ganhou a primeira partida",
            icone: "🃏",
        },
        |j| j.vitorias >= 1,
    ),
    (
        Emblema {
            chave: "trucador",
            nome: "Trucador",
            descricao: "cinco vitórias",
            icone: "⭐",
        },
        |j| j.vitorias >= 5,
    ),
    (
        Emblema {
            chave: "mao_quente",
            nome: "Mão quente",
            descricao: "três vitórias seguidas em algum momento",
            icone: "🔥",
        },
        |j| j.melhor_sequencia >= 3,
    ),
    (
        Emblema {
            chave: "veterano",
            nome: "Veterano",
            descricao: "vinte partidas disputadas",
            icone: "🛡️",
        },
        |j| j.partidas >= 20,
    ),
    (
        Emblema {
            chave: "lavada",
            nome: "Lavada",
            descricao: "ganhou uma partida de doze a zero",
            icone: "💎",
        },
        |j| j.lavadas >= 1,
    ),
];

pub fn de(j: &Jogador) -> Vec<Emblema> {
    TODOS.iter().filter(|(_, criterio)| criterio(j)).map(|(e, _)| e.clone()).collect()
}
