//! A máquina de estados de uma partida de truco paulista.
//!
//! Pura: não sabe o que é rede, banco nem tempo. Recebe uma ação de um assento e devolve o
//! que aconteceu, ou um erro. Toda regra citada como `R-nn` está em
//! `regras/truco-paulista.md` do repositório de pesquisa, com a fonte.

use crate::carta::{Carta, Numero};
use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};

/// Quantos jogadores na mesa. O truco paulista das fontes é 2x2; o 1x1 é decisão nossa
/// (R-28..R-31) e consiste em tratar cada jogador como uma dupla de um.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modo {
    UmContraUm,
    DoisContraDois,
}

impl Modo {
    pub const fn assentos(self) -> usize {
        match self {
            Modo::UmContraUm => 2,
            Modo::DoisContraDois => 4,
        }
    }
}

/// Dupla 0 = assentos pares, dupla 1 = assentos ímpares. Com 4 assentos isso põe o parceiro
/// à frente, como as fontes exigem; com 2, cada um é a sua própria dupla.
pub const fn equipe_de(assento: usize) -> u8 {
    (assento % 2) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoMao {
    /// Mão comum, começa valendo 1.
    Normal,
    /// Uma dupla está com 11 e decide se joga (R-22).
    Onze { equipe: u8 },
    /// As duas duplas com 11: vale 1, sem truco, e quem ganhar leva a partida (R-26).
    Ferro,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Jogada {
    pub assento: usize,
    /// A carta que saiu da mão. **Não** vai para o adversário quando `coberta`.
    #[serde(skip)]
    pub carta: Carta,
    /// Jogada de costas (R-14): não é revelada e não disputa a rodada.
    pub coberta: bool,
}

/// Uma jogada como qualquer um pode vê-la. É o que sai no protocolo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JogadaVisivel {
    pub assento: usize,
    /// `None` quando a carta foi jogada de costas. **Fica `None` para sempre**: a regra diz
    /// que o valor não é revelado, e isso não expira quando a rodada fecha (R-14).
    pub carta: Option<Carta>,
    pub coberta: bool,
}

impl From<Jogada> for JogadaVisivel {
    fn from(j: Jogada) -> Self {
        JogadaVisivel {
            assento: j.assento,
            carta: (!j.coberta).then_some(j.carta),
            coberta: j.coberta,
        }
    }
}

/// Uma rodada já resolvida, com as cartas e quem levou.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RodadaVisivel {
    /// `None` = empatou.
    pub vencedora: Option<u8>,
    /// O assento que pôs a carta que levou a rodada. `None` quando empatou.
    ///
    /// Vem do servidor porque o cliente **não pode** derivá-lo: saber qual carta venceu
    /// exige a ordem de força e a manilha, e o cliente não conhece as regras de propósito
    /// (ADR-003). Sem este campo a interface só podia dizer "a dupla X levou", nunca "foi
    /// esta carta" — e foi o próprio construtor da tela quem apontou a falta.
    pub assento_vencedor: Option<usize>,
    /// Na ordem em que as cartas foram à mesa.
    pub jogadas: Vec<JogadaVisivel>,
}

// Sem `Deserialize`: `Rodada` é estado interno e nunca volta do protocolo — o que trafega
// é `RodadaVisivel`, que esconde a carta de costas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rodada {
    /// `None` = rodada empatada (R-03, R-11).
    pub vencedor: Option<u8>,
    /// Quem puxa a rodada seguinte (R-12).
    pub puxador_seguinte: usize,
    /// As jogadas desta rodada, na ordem.
    ///
    /// Guardadas porque a rodada resolvida **precisa continuar visível**. Antes a rodada só
    /// levava o vencedor, e o servidor limpava a mesa ao resolvê-la: a carta do adversário
    /// aparecia e desaparecia entre dois quadros, e nem um cliente perfeito conseguiria
    /// mostrar a rodada, porque o dado não existia no protocolo.
    pub jogadas: Vec<Jogada>,
}

impl Rodada {
    pub fn visivel(&self) -> RodadaVisivel {
        RodadaVisivel {
            vencedora: self.vencedor,
            // `puxador_seguinte` é, por R-12, quem pôs a carta do topo: o vencedor quando
            // há vencedor, e o primeiro do empate quando não há. Só é o vencedor no
            // primeiro caso, então o segundo não vira informação falsa.
            assento_vencedor: self.vencedor.map(|_| self.puxador_seguinte),
            jogadas: self
                .jogadas
                .iter()
                .copied()
                .map(JogadaVisivel::from)
                .collect(),
        }
    }
}

/// Um pedido de truco/seis/nove/doze esperando resposta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pendencia {
    pub equipe_pedinte: u8,
    pub assento_pedinte: usize,
    /// Quem deve responder — um só, por ADR-005.
    pub assento_respondente: usize,
    /// Quanto a mão passa a valer se aceitarem.
    pub valor_proposto: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "como")]
pub enum FimDaMao {
    /// Disputada até o fim nas cartas.
    Cartas { vencedora: Option<u8>, pontos: u8 },
    /// Alguém correu de um pedido (R-18).
    Correu { vencedora: u8, pontos: u8 },
    /// Mão de onze recusada (R-23).
    OnzeRecusada { vencedora: u8, pontos: u8 },
}

impl FimDaMao {
    pub fn vencedora(self) -> Option<u8> {
        match self {
            FimDaMao::Cartas { vencedora, .. } => vencedora,
            FimDaMao::Correu { vencedora, .. } | FimDaMao::OnzeRecusada { vencedora, .. } => {
                Some(vencedora)
            }
        }
    }
    pub fn pontos(self) -> u8 {
        match self {
            FimDaMao::Cartas { pontos, .. }
            | FimDaMao::Correu { pontos, .. }
            | FimDaMao::OnzeRecusada { pontos, .. } => pontos,
        }
    }
}

/// O que um jogador pode tentar fazer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "acao")]
pub enum Acao {
    /// Põe a `indice`-ésima carta da mão na mesa. `coberta` só na 2ª e 3ª rodadas (R-14).
    Jogar {
        indice: usize,
        coberta: bool,
    },
    /// Pede truco, ou o aumento seguinte na escada (R-15).
    Pedir,
    Aceitar,
    Correr,
    /// Aceita e retruca de uma vez (R-17).
    Aumentar,
    /// Resposta da mão de onze (R-22).
    Onze {
        aceita: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "erro", content = "detalhe")]
pub enum Erro {
    PartidaTerminada,
    NaoEhSuaVez,
    IndiceDeCartaInvalido,
    /// R-14: não se esconde carta na primeira rodada.
    CobertaNaPrimeiraRodada,
    /// Há um pedido de truco na mesa; ninguém joga carta antes de responder.
    RespondaOPedido,
    NaoHaPedidoParaResponder,
    /// R-15: a escada já está em 12.
    ValorNoTeto,
    /// R-19: a dupla que fez o último pedido não faz o seguinte.
    PedidoSeguidoDaMesmaDupla,
    /// R-20: no doze só se aceita ou corre.
    NoDozeNaoSeAumenta,
    /// R-24 / R-26: mão de onze e mão de ferro não admitem pedido.
    PedidoProibidoNestaMao,
    /// A mão de onze precisa ser respondida antes da primeira carta (R-22).
    DecidaAMaoDeOnze,
    NaoEhMaoDeOnze,
    /// Só a dupla que está com 11 responde a mão de onze.
    NaoEhSuaMaoDeOnze,
}

impl std::fmt::Display for Erro {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Erro::PartidaTerminada => "a partida já terminou",
            Erro::NaoEhSuaVez => "não é a sua vez",
            Erro::IndiceDeCartaInvalido => "você não tem essa carta",
            Erro::CobertaNaPrimeiraRodada => "não se esconde carta na primeira rodada",
            Erro::RespondaOPedido => "responda o pedido antes de jogar",
            Erro::NaoHaPedidoParaResponder => "não há pedido para responder",
            Erro::ValorNoTeto => "a mão já vale doze",
            Erro::PedidoSeguidoDaMesmaDupla => "a sua dupla fez o último pedido",
            Erro::NoDozeNaoSeAumenta => "no doze só dá para aceitar ou correr",
            Erro::PedidoProibidoNestaMao => "esta mão não admite pedido de truco",
            Erro::DecidaAMaoDeOnze => "decida a mão de onze antes",
            Erro::NaoEhMaoDeOnze => "esta não é uma mão de onze",
            Erro::NaoEhSuaMaoDeOnze => "a mão de onze não é da sua dupla",
        };
        f.write_str(s)
    }
}

impl std::error::Error for Erro {}

/// Uma linha de narração do que acabou de acontecer. O servidor repassa para a mesa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "aviso")]
pub enum Aviso {
    Jogou {
        assento: usize,
        carta: Option<char>,
    },
    RodadaResolvida {
        indice: usize,
        vencedora: Option<u8>,
        puxador_seguinte: usize,
    },
    Pediu {
        assento: usize,
        valor_proposto: u8,
    },
    Aceitou {
        assento: usize,
        valor: u8,
    },
    Correu {
        assento: usize,
    },
    MaoTerminou {
        fim: FimDaMao,
        placar: [u8; 2],
    },
    /// O detalhe inteiro da mão que acabou: as rodadas com as cartas, como ela terminou, e
    /// o placar depois dela.
    ///
    /// Existe porque o `estado` seguinte já é da **mão nova** — sem este aviso, a última
    /// rodada de cada mão se perderia, que é exatamente o defeito que o jogador relatou.
    MaoResolvida {
        numero: u32,
        vira: Carta,
        /// O número das manilhas, como rótulo ("4", "Q", "A"), nunca como carta.
        manilha: String,
        /// Quanto a mão valia ao terminar: 1, 3, 6, 9 ou 12 (R-15). Com isto o histórico
        /// filtra "mãos com truco" por `valor > 1`, em vez de reconstruir a partir dos
        /// avisos de pedido — reconstrução que falha se um aviso se perder ou chegar fora
        /// de ordem. Quem apontou a falta foi quem construiu a tela.
        valor: u8,
        rodadas: Vec<RodadaVisivel>,
        fim: FimDaMao,
        placar: [u8; 2],
    },
    MaoDeOnze {
        equipe: u8,
    },
    OnzeRespondida {
        equipe: u8,
        aceita: bool,
    },
    MaoDeFerro,
    MaoComecou {
        numero: u32,
        tipo: TipoMao,
    },
    PartidaTerminou {
        vencedora: u8,
    },
}

/// A mão em curso.
#[derive(Debug, Clone)]
pub struct Mao {
    pub vira: Carta,
    pub manilha: Numero,
    /// Cartas ainda na mão de cada assento.
    pub cartas: Vec<Vec<Carta>>,
    pub mesa: Vec<Jogada>,
    pub rodadas: Vec<Rodada>,
    pub puxador: usize,
    pub vez: usize,
    pub valor: u8,
    pub tipo: TipoMao,
    pub pendencia: Option<Pendencia>,
    /// Dupla que fez o último pedido nesta mão (R-19). Leitura pública; só `pedir` escreve.
    pub ultima_equipe_pedinte: Option<u8>,
    onze_respondida: bool,
    pub fim: Option<FimDaMao>,
}

impl Mao {
    /// Distribui uma mão nova, embaralhando. `placar` decide se é mão comum, de onze ou de
    /// ferro.
    pub fn nova(modo: Modo, puxador: usize, placar: [u8; 2], rng: &mut impl Rng) -> Mao {
        let mut baralho = Carta::baralho();
        embaralhar(&mut baralho, rng);
        Mao::do_baralho(modo, puxador, placar, &baralho)
    }

    /// A mesma distribuição, a partir de um baralho **já ordenado por quem chama**: as
    /// primeiras `3n` cartas são as mãos (três a três, por assento) e a `3n`-ésima é a vira.
    ///
    /// Existe para que o teste de regra seja determinístico — e é o mesmo caminho que um
    /// replay de partida usaria. `nova` é só isto com um embaralhamento antes.
    pub fn do_baralho(modo: Modo, puxador: usize, placar: [u8; 2], baralho: &[Carta]) -> Mao {
        let n = modo.assentos();
        assert!(
            baralho.len() > n * 3,
            "baralho curto: precisa de 3n cartas e a vira"
        );

        let cartas: Vec<Vec<Carta>> = (0..n).map(|i| baralho[i * 3..i * 3 + 3].to_vec()).collect();
        // A vira sai depois das mãos; o resto do monte não é mais usado (R-28).
        let vira = baralho[n * 3];
        let manilha = vira.numero.seguinte();

        let tipo = match (placar[0] >= 11, placar[1] >= 11) {
            (true, true) => TipoMao::Ferro,
            (true, false) => TipoMao::Onze { equipe: 0 },
            (false, true) => TipoMao::Onze { equipe: 1 },
            (false, false) => TipoMao::Normal,
        };
        // R-23: a mão de onze aceita já começa valendo 3. R-26: a de ferro vale 1.
        let valor = if matches!(tipo, TipoMao::Onze { .. }) {
            3
        } else {
            1
        };

        Mao {
            vira,
            manilha,
            cartas,
            mesa: Vec::new(),
            rodadas: Vec::new(),
            puxador,
            vez: puxador,
            valor,
            tipo,
            pendencia: None,
            ultima_equipe_pedinte: None,
            onze_respondida: false,
            fim: None,
        }
    }

    pub fn indice_da_rodada(&self) -> usize {
        self.rodadas.len()
    }

    /// `true` enquanto a dupla de onze não respondeu (R-22: antes da primeira carta).
    pub fn aguarda_onze(&self) -> bool {
        matches!(self.tipo, TipoMao::Onze { .. })
            && self.rodadas.is_empty()
            && self.mesa.is_empty()
            && self.fim.is_none()
            && !self.onze_respondida
    }

    fn proximo(&self, assento: usize) -> usize {
        (assento + 1) % self.cartas.len()
    }
}

/// Fisher-Yates com o RNG que vier. O embaralhamento decide dinheiro de jogo, então quem
/// chama é obrigado a passar um CSPRNG (ver ADR-001); aqui a função só não escolhe por você.
fn embaralhar(cartas: &mut [Carta], rng: &mut impl Rng) {
    for i in (1..cartas.len()).rev() {
        cartas.swap(i, rng.random_range(0..=i));
    }
}

/// Próximo degrau da escada do truco (R-15). Não há outros valores.
pub const fn proximo_valor(valor: u8) -> Option<u8> {
    match valor {
        1 => Some(3),
        3 => Some(6),
        6 => Some(9),
        9 => Some(12),
        _ => None,
    }
}

/// Quem ganhou a rodada que está na mesa (R-03, R-07, R-12, R-14).
pub fn resolver_rodada(mesa: &[Jogada], manilha: Numero) -> Rodada {
    // Cartas de costas não disputam (R-14).
    let disputando: Vec<&Jogada> = mesa.iter().filter(|j| !j.coberta).collect();

    // [DECISÃO] todas de costas: ninguém disputa, logo empate. Nenhuma fonte trata o caso.
    let jogadas = mesa.to_vec();
    let Some(forca_maxima) = disputando.iter().map(|j| j.carta.forca(manilha)).max() else {
        return Rodada {
            vencedor: None,
            puxador_seguinte: mesa[0].assento,
            jogadas,
        };
    };

    let no_topo: Vec<&&Jogada> = disputando
        .iter()
        .filter(|j| j.carta.forca(manilha) == forca_maxima)
        .collect();

    // "quem pôs na mesa a primeira carta que empatou" (R-12) — a ordem de `mesa` é a ordem
    // de jogada, então o primeiro do topo é literalmente esse jogador.
    let primeiro_do_topo = no_topo[0].assento;
    let equipes: Vec<u8> = no_topo.iter().map(|j| equipe_de(j.assento)).collect();

    if equipes.iter().all(|e| *e == equipes[0]) {
        // Inclui o caso de dois parceiros empatarem no topo: a dupla ganhou a rodada.
        Rodada {
            vencedor: Some(equipes[0]),
            puxador_seguinte: primeiro_do_topo,
            jogadas,
        }
    } else {
        Rodada {
            vencedor: None,
            puxador_seguinte: primeiro_do_topo,
            jogadas,
        }
    }
}

/// Quem leva a mão, dadas as rodadas já resolvidas (R-10, R-11).
///
/// `None` = ainda indefinido. `Some(None)` = as três empataram, ninguém pontua.
pub fn decidir_mao(rodadas: &[Rodada]) -> Option<Option<u8>> {
    let v: Vec<Option<u8>> = rodadas.iter().map(|r| r.vencedor).collect();
    match v.len() {
        0 | 1 => None,
        2 => match (v[0], v[1]) {
            // "vencer uma e empatar outra" (R-10), nas duas ordens.
            (None, Some(t)) | (Some(t), None) => Some(Some(t)),
            // Duas rodadas para a mesma dupla.
            (Some(a), Some(b)) if a == b => Some(Some(a)),
            // Uma para cada, ou as duas empatadas: vai para a terceira.
            _ => None,
        },
        _ => match (v[0], v[1], v[2]) {
            // 1ª e 2ª empatadas: a 3ª decide — e se ela também empatar, ninguém pontua.
            (None, None, w) => Some(w),
            // Empate na 3ª com a 1ª decidida: leva quem venceu a 1ª.
            (Some(a), Some(_), None) => Some(Some(a)),
            (_, _, Some(t)) => Some(Some(t)),
            _ => Some(None),
        },
    }
}

/// A partida inteira: placar, mão corrente, e o vencedor quando houver.
#[derive(Debug, Clone)]
pub struct Partida {
    pub modo: Modo,
    pub placar: [u8; 2],
    pub numero_da_mao: u32,
    pub mao: Mao,
    pub vencedora: Option<u8>,
}

/// Pontos para encerrar a partida (R-27).
pub const ALVO: u8 = 12;

impl Partida {
    pub fn nova(modo: Modo, rng: &mut impl Rng) -> Partida {
        Partida {
            modo,
            placar: [0, 0],
            numero_da_mao: 0,
            mao: Mao::nova(modo, 0, [0, 0], rng),
            vencedora: None,
        }
    }

    /// Partida montada numa situação escolhida: placar e baralho dados. Para teste de
    /// regra e para replay; `nova` é este com placar zerado e baralho embaralhado.
    pub fn com_baralho(modo: Modo, placar: [u8; 2], baralho: &[Carta]) -> Partida {
        Partida {
            modo,
            placar,
            numero_da_mao: 0,
            mao: Mao::do_baralho(modo, 0, placar, baralho),
            vencedora: None,
        }
    }

    pub fn assentos(&self) -> usize {
        self.modo.assentos()
    }

    /// Aplica a ação de um assento. Erro = nada mudou.
    pub fn aplicar(
        &mut self,
        assento: usize,
        acao: Acao,
        rng: &mut impl Rng,
    ) -> Result<Vec<Aviso>, Erro> {
        if self.vencedora.is_some() {
            return Err(Erro::PartidaTerminada);
        }
        if assento >= self.assentos() {
            return Err(Erro::NaoEhSuaVez);
        }
        let mut avisos = Vec::new();
        match acao {
            Acao::Onze { aceita } => self.responder_onze(assento, aceita, &mut avisos, rng)?,
            Acao::Pedir => self.pedir(assento, &mut avisos)?,
            Acao::Aceitar => self.aceitar(assento, &mut avisos)?,
            Acao::Correr => self.correr(assento, &mut avisos, rng)?,
            Acao::Aumentar => self.aumentar(assento, &mut avisos)?,
            Acao::Jogar { indice, coberta } => {
                self.jogar(assento, indice, coberta, &mut avisos, rng)?
            }
        }
        Ok(avisos)
    }

    fn responder_onze(
        &mut self,
        assento: usize,
        aceita: bool,
        avisos: &mut Vec<Aviso>,
        rng: &mut impl Rng,
    ) -> Result<(), Erro> {
        if !self.mao.aguarda_onze() {
            return Err(Erro::NaoEhMaoDeOnze);
        }
        let TipoMao::Onze { equipe } = self.mao.tipo else {
            return Err(Erro::NaoEhMaoDeOnze);
        };
        // R-22 é decisão da dupla; qualquer um dos dois responde, e vale a primeira resposta.
        // Aqui não há corrida de rede como no truco (ADR-005): não existe vez a perder.
        if equipe_de(assento) != equipe {
            return Err(Erro::NaoEhSuaMaoDeOnze);
        }
        self.mao.onze_respondida = true;
        avisos.push(Aviso::OnzeRespondida { equipe, aceita });
        if !aceita {
            // R-23: correu, a dupla adversária recebe 1 ponto.
            let fim = FimDaMao::OnzeRecusada {
                vencedora: 1 - equipe,
                pontos: 1,
            };
            self.encerrar_mao(fim, avisos, rng);
        }
        Ok(())
    }

    fn pedir(&mut self, assento: usize, avisos: &mut Vec<Aviso>) -> Result<(), Erro> {
        if self.mao.aguarda_onze() {
            return Err(Erro::DecidaAMaoDeOnze);
        }
        if self.mao.pendencia.is_some() {
            return Err(Erro::RespondaOPedido);
        }
        // R-24 e R-26: nem a mão de onze nem a de ferro admitem pedido.
        if !matches!(self.mao.tipo, TipoMao::Normal) {
            return Err(Erro::PedidoProibidoNestaMao);
        }
        if self.mao.vez != assento {
            return Err(Erro::NaoEhSuaVez);
        }
        let equipe = equipe_de(assento);
        // R-15 antes de R-19 de propósito: "a mão já vale doze" é a razão estrutural, e é a
        // que o jogador precisa ouvir. Dizer "a sua dupla pediu o último" num 12 seria uma
        // verdade que não explica nada.
        let valor_proposto = proximo_valor(self.mao.valor).ok_or(Erro::ValorNoTeto)?;
        // R-19: não se pede duas vezes seguidas pela mesma dupla.
        if self.mao.ultima_equipe_pedinte == Some(equipe) {
            return Err(Erro::PedidoSeguidoDaMesmaDupla);
        }
        self.mao.pendencia = Some(Pendencia {
            equipe_pedinte: equipe,
            assento_pedinte: assento,
            // ADR-005: um respondente só, e é o próximo adversário no assento.
            assento_respondente: self.mao.proximo(assento),
            valor_proposto,
        });
        self.mao.ultima_equipe_pedinte = Some(equipe);
        avisos.push(Aviso::Pediu {
            assento,
            valor_proposto,
        });
        Ok(())
    }

    fn pendencia_de(&self, assento: usize) -> Result<Pendencia, Erro> {
        let p = self.mao.pendencia.ok_or(Erro::NaoHaPedidoParaResponder)?;
        if p.assento_respondente != assento {
            return Err(Erro::NaoEhSuaVez);
        }
        Ok(p)
    }

    fn aceitar(&mut self, assento: usize, avisos: &mut Vec<Aviso>) -> Result<(), Erro> {
        let p = self.pendencia_de(assento)?;
        self.mao.valor = p.valor_proposto;
        self.mao.pendencia = None;
        // **`vez` não se move durante a negociação do truco.** Ela já aponta para quem está
        // devendo a carta, e `jogar` recusa enquanto houver pendência, então não há nada a
        // proteger mudando-a.
        //
        // Antes isto fazia `vez = p.assento_pedinte`, e estava errado numa cadeia de
        // retrucos de tamanho par: depois de "A pede truco, B pede seis, A aceita", o
        // `assento_pedinte` da pendência corrente é B — e a vez ia para B, que não devia
        // carta nenhuma. Dali em diante a ordem de jogada corrompia, e as mãos chegavam a
        // contagens impossíveis (um assento com 2 cartas e os outros com 0).
        //
        // Achado pelo teste de propriedade do bot, não pelos testes de regra: eles conferiam
        // o `valor` da escada e nunca de quem era a vez depois dela.
        avisos.push(Aviso::Aceitou {
            assento,
            valor: self.mao.valor,
        });
        Ok(())
    }

    fn correr(
        &mut self,
        assento: usize,
        avisos: &mut Vec<Aviso>,
        rng: &mut impl Rng,
    ) -> Result<(), Erro> {
        let p = self.pendencia_de(assento)?;
        avisos.push(Aviso::Correu { assento });
        // R-18: quem corre entrega o valor que a mão tinha ANTES do pedido.
        let fim = FimDaMao::Correu {
            vencedora: p.equipe_pedinte,
            pontos: self.mao.valor,
        };
        self.encerrar_mao(fim, avisos, rng);
        Ok(())
    }

    fn aumentar(&mut self, assento: usize, avisos: &mut Vec<Aviso>) -> Result<(), Erro> {
        let p = self.pendencia_de(assento)?;
        // R-20: o doze não se aumenta.
        let proposto = proximo_valor(p.valor_proposto).ok_or(Erro::NoDozeNaoSeAumenta)?;
        // Retrucar é aceitar o degrau atual e propor o seguinte (R-17).
        self.mao.valor = p.valor_proposto;
        let equipe = equipe_de(assento);
        self.mao.pendencia = Some(Pendencia {
            equipe_pedinte: equipe,
            assento_pedinte: assento,
            // A conversa volta para quem começou: é o adversário, e é quem tem o contexto.
            assento_respondente: p.assento_pedinte,
            valor_proposto: proposto,
        });
        self.mao.ultima_equipe_pedinte = Some(equipe);
        avisos.push(Aviso::Aceitou {
            assento,
            valor: self.mao.valor,
        });
        avisos.push(Aviso::Pediu {
            assento,
            valor_proposto: proposto,
        });
        Ok(())
    }

    fn jogar(
        &mut self,
        assento: usize,
        indice: usize,
        coberta: bool,
        avisos: &mut Vec<Aviso>,
        rng: &mut impl Rng,
    ) -> Result<(), Erro> {
        if self.mao.aguarda_onze() {
            return Err(Erro::DecidaAMaoDeOnze);
        }
        if self.mao.pendencia.is_some() {
            return Err(Erro::RespondaOPedido);
        }
        if self.mao.vez != assento {
            return Err(Erro::NaoEhSuaVez);
        }
        // R-14: carta de costas só na 2ª e na 3ª rodada.
        if coberta && self.mao.rodadas.is_empty() {
            return Err(Erro::CobertaNaPrimeiraRodada);
        }
        let mao_do_jogador = &mut self.mao.cartas[assento];
        if indice >= mao_do_jogador.len() {
            return Err(Erro::IndiceDeCartaInvalido);
        }
        let carta = mao_do_jogador.remove(indice);
        self.mao.mesa.push(Jogada {
            assento,
            carta,
            coberta,
        });
        avisos.push(Aviso::Jogou {
            assento,
            // A carta de costas não vai no aviso: quem está de fora não pode sabê-la.
            carta: (!coberta).then(|| carta.unicode()),
        });

        if self.mao.mesa.len() < self.assentos() {
            self.mao.vez = self.mao.proximo(assento);
            return Ok(());
        }

        let rodada = resolver_rodada(&self.mao.mesa, self.mao.manilha);
        // `Rodada` deixou de ser `Copy` quando passou a guardar as jogadas, então o que
        // ainda é preciso depois do `push` é copiado antes dele.
        let (vencedora, puxador_seguinte) = (rodada.vencedor, rodada.puxador_seguinte);
        self.mao.mesa.clear();
        self.mao.rodadas.push(rodada);
        avisos.push(Aviso::RodadaResolvida {
            indice: self.mao.rodadas.len() - 1,
            vencedora,
            puxador_seguinte,
        });

        match decidir_mao(&self.mao.rodadas) {
            Some(vencedora) => {
                let pontos = if vencedora.is_some() {
                    self.mao.valor
                } else {
                    0
                };
                let fim = FimDaMao::Cartas { vencedora, pontos };
                self.encerrar_mao(fim, avisos, rng);
            }
            None => self.mao.vez = puxador_seguinte,
        }
        Ok(())
    }

    fn encerrar_mao(&mut self, fim: FimDaMao, avisos: &mut Vec<Aviso>, rng: &mut impl Rng) {
        self.mao.fim = Some(fim);
        // O detalhe primeiro, o resumo depois: quem processa os avisos em ordem monta o
        // histórico e só então lê "a mão terminou".
        avisos.push(Aviso::MaoResolvida {
            numero: self.numero_da_mao,
            vira: self.mao.vira,
            manilha: self.mao.manilha.rotulo().to_string(),
            valor: self.mao.valor,
            rodadas: self.mao.rodadas.iter().map(Rodada::visivel).collect(),
            fim,
            placar: {
                // O placar **depois** desta mão: soma aqui porque o crédito abaixo ainda
                // não aconteceu.
                let mut p = self.placar;
                if let Some(e) = fim.vencedora() {
                    p[e as usize] = p[e as usize].saturating_add(fim.pontos());
                }
                p
            },
        });
        if let Some(e) = fim.vencedora() {
            self.placar[e as usize] = self.placar[e as usize].saturating_add(fim.pontos());
        }
        avisos.push(Aviso::MaoTerminou {
            fim,
            placar: self.placar,
        });

        // R-27: chega ou passa de 12 e a partida acabou.
        if let Some(e) = (0..2u8).find(|e| self.placar[*e as usize] >= ALVO) {
            self.vencedora = Some(e);
            avisos.push(Aviso::PartidaTerminou { vencedora: e });
            return;
        }

        self.numero_da_mao += 1;
        // R-13: o direito de puxar anda um assento por mão.
        let puxador = (self.numero_da_mao as usize) % self.assentos();
        self.mao = Mao::nova(self.modo, puxador, self.placar, rng);
        avisos.push(Aviso::MaoComecou {
            numero: self.numero_da_mao,
            tipo: self.mao.tipo,
        });
        match self.mao.tipo {
            TipoMao::Onze { equipe } => avisos.push(Aviso::MaoDeOnze { equipe }),
            TipoMao::Ferro => avisos.push(Aviso::MaoDeFerro),
            TipoMao::Normal => {}
        }
    }
}
