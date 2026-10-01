//! A mesa: uma tarefa que é dona de uma `Partida` e fala com N sockets.
//!
//! Por que uma tarefa dona do estado, em vez de `Mutex<Partida>` compartilhado: a partida é
//! uma máquina de estados sequencial — duas ações nunca devem ser aplicadas em paralelo. Uma
//! tarefa com uma fila de comandos **é** essa serialização, e sem travar nada.

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use truco_regras::visao::Visao;
use truco_regras::{Acao, Aviso, Modo, Partida};

use crate::bd::{self, Jogador};
use crate::estado::{Espera, Estado};
use crate::webhooks;

/// Mensagem do servidor para um cliente.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ParaCliente {
    Fila {
        modo: &'static str,
        aposta: i64,
        faltam: usize,
    },
    Mesa {
        partida: String,
        assento: usize,
        aposta: i64,
        /// Mesa de treino: tem bot, não vale moeda e não conta para o ranking.
        treino: bool,
        jogadores: Vec<NaMesa>,
    },
    Estado(Visao),
    Avisos {
        avisos: Vec<Aviso>,
    },
    Erro {
        erro: &'static str,
        mensagem: String,
    },
    Fim {
        vencedora: u8,
        placar: [u8; 2],
        moedas: i64,
        ganho: i64,
        treino: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct NaMesa {
    pub assento: usize,
    pub apelido: String,
    pub equipe: u8,
    pub bot: bool,
}

/// Mensagem de um cliente para a mesa.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum DoCliente {
    Jogar {
        indice: usize,
        #[serde(default)]
        coberta: bool,
    },
    Pedir,
    Aceitar,
    Correr,
    Aumentar,
    Onze {
        aceita: bool,
    },
}

impl From<DoCliente> for Acao {
    fn from(m: DoCliente) -> Acao {
        match m {
            DoCliente::Jogar { indice, coberta } => Acao::Jogar { indice, coberta },
            DoCliente::Pedir => Acao::Pedir,
            DoCliente::Aceitar => Acao::Aceitar,
            DoCliente::Correr => Acao::Correr,
            DoCliente::Aumentar => Acao::Aumentar,
            DoCliente::Onze { aceita } => Acao::Onze { aceita },
        }
    }
}

pub enum Comando {
    Agir(usize, Acao),
    /// O socket daquele assento caiu.
    Saiu(usize),
}

pub fn nome_do_modo(m: Modo) -> &'static str {
    match m {
        Modo::UmContraUm => "1x1",
        Modo::DoisContraDois => "2x2",
    }
}

/// O que não muda durante a mesa. Existe para que `encerrar` receba um contexto em vez de
/// oito parâmetros posicionais, onde trocar dois de lugar compila e paga errado.
struct Contexto {
    partida_id: String,
    modo: Modo,
    aposta: i64,
    /// Mesa com bot. Não paga, não pontua, não entra no ranking.
    treino: bool,
}

/// Quem ocupa o assento. O bot não tem ficha no banco de propósito: ele não tem saldo a
/// mover nem estatística a fechar, e o tipo é o que impede de esquecer isso num `if`.
enum Ocupante {
    Humano(Jogador),
    Bot { apelido: String },
}

impl Ocupante {
    fn apelido(&self) -> &str {
        match self {
            Ocupante::Humano(j) => &j.apelido,
            Ocupante::Bot { apelido } => apelido,
        }
    }
    fn eh_bot(&self) -> bool {
        matches!(self, Ocupante::Bot { .. })
    }
    /// A ficha, quando houver. `None` para bot — e é por isso que pagamento e estatística
    /// não têm como alcançá-lo.
    fn humano(&self) -> Option<&Jogador> {
        match self {
            Ocupante::Humano(j) => Some(j),
            Ocupante::Bot { .. } => None,
        }
    }
}

struct Cadeira {
    ocupante: Ocupante,
    canal: mpsc::UnboundedSender<ParaCliente>,
    /// O socket já foi embora? Mesa não morre por causa de um `send` para quem saiu.
    /// Bot está sempre vivo: não há socket para cair.
    vivo: bool,
}

/// Monta a mesa e roda a partida até o fim. Consome as esperas, e **preenche com bot o que
/// faltar** — o que só acontece em mesa de treino, porque a fila comum só chama isto com a
/// mesa cheia.
pub fn abrir(estado: Estado, modo: Modo, aposta: i64, esperas: Vec<Espera>, treino: bool) {
    tokio::spawn(async move {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let mut cadeiras = Vec::new();
        for (assento, e) in esperas.into_iter().enumerate() {
            // Se o socket morreu entre entrar na fila e a mesa abrir, este envio falha — e
            // aí a mesa começa com um assento morto, que o laço trata como abandono.
            let vivo = e.entrou_na_mesa.send((assento, cmd_tx.clone())).is_ok();
            cadeiras.push(Cadeira {
                ocupante: Ocupante::Humano(e.jogador),
                canal: e.para_cliente,
                vivo,
            });
        }
        for assento in cadeiras.len()..modo.assentos() {
            // O canal do bot é o mesmo tipo do canal de um humano, e o bot recebe por ele a
            // mesma `Visao`. É isso que o torna incapaz de ver carta alheia (ver `bot.rs`).
            let (para_bot, do_bot) = mpsc::unbounded_channel();
            crate::bot::soltar(assento, do_bot, cmd_tx.clone(), estado.config.pausa_do_bot);
            cadeiras.push(Cadeira {
                ocupante: Ocupante::Bot {
                    apelido: crate::bot::NOMES[(assento - 1) % crate::bot::NOMES.len()].to_string(),
                },
                canal: para_bot,
                vivo: true,
            });
        }
        if let Err(e) = rodar(estado, modo, aposta, cadeiras, cmd_rx, treino).await {
            tracing::error!(erro = %e, "mesa terminou com erro");
        }
    });
}

async fn rodar(
    estado: Estado,
    modo: Modo,
    aposta: i64,
    mut cadeiras: Vec<Cadeira>,
    mut cmd_rx: mpsc::UnboundedReceiver<Comando>,
    treino: bool,
) -> anyhow::Result<()> {
    let partida_id = uuid::Uuid::now_v7().to_string();
    let na_mesa: Vec<NaMesa> = cadeiras
        .iter()
        .enumerate()
        .map(|(assento, c)| NaMesa {
            assento,
            apelido: c.ocupante.apelido().to_string(),
            equipe: truco_regras::equipe_de(assento),
            bot: c.ocupante.eh_bot(),
        })
        .collect();

    sqlx::query("INSERT INTO partidas (id, modo, aposta, comecou_em) VALUES (?1, ?2, ?3, ?4)")
        .bind(&partida_id)
        .bind(nome_do_modo(modo))
        .bind(aposta)
        .bind(bd::agora())
        .execute(&estado.pool)
        .await?;

    let ctx = Contexto {
        partida_id: partida_id.clone(),
        modo,
        aposta,
        treino,
    };
    let mut partida = Partida::nova(modo, &mut rand::rng());

    for (assento, c) in cadeiras.iter().enumerate() {
        let _ = c.canal.send(ParaCliente::Mesa {
            partida: partida_id.clone(),
            assento,
            aposta,
            treino,
            jogadores: na_mesa.clone(),
        });
    }
    mandar_estado(&cadeiras, &partida);

    webhooks::disparar(
        estado.pool.clone(),
        estado.cliente.clone(),
        humanos(&cadeiras),
        "partida.comecou",
        serde_json::json!({
            "evento": "partida.comecou",
            "em": bd::agora(),
            "partida": {
                "id": partida_id,
                "modo": nome_do_modo(modo),
                "aposta": aposta,
                "treino": treino,
                "jogadores": na_mesa,
            }
        }),
    );

    // Um assento que já nasceu morto encerra a partida antes do primeiro comando.
    if let Some(morto) = cadeiras.iter().position(|c| !c.vivo) {
        let vencedora = 1 - truco_regras::equipe_de(morto) as u8;
        return encerrar(&estado, &ctx, &cadeiras, &partida, vencedora, true).await;
    }

    loop {
        let cmd = match tokio::time::timeout(estado.config.prazo_de_jogada, cmd_rx.recv()).await {
            Ok(Some(c)) => c,
            // Todos os canais fecharam: não há mais ninguém para quem perguntar.
            Ok(None) => return Ok(()),
            Err(_) => {
                // Ninguém agiu no prazo. Perde a dupla de quem estava devendo a ação — e a
                // mesa fecha, que é o ponto: o dinheiro do outro não fica preso.
                let devedor = quem_deve_agir(&partida);
                difundir(
                    &cadeiras,
                    ParaCliente::Erro {
                        erro: "tempo_esgotado",
                        mensagem: format!(
                            "ninguém jogou em {}s; a partida foi encerrada",
                            estado.config.prazo_de_jogada.as_secs()
                        ),
                    },
                );
                let vencedora = 1 - truco_regras::equipe_de(devedor);
                return encerrar(&estado, &ctx, &cadeiras, &partida, vencedora, true).await;
            }
        };
        match cmd {
            Comando::Agir(assento, acao) => {
                match partida.aplicar(assento, acao, &mut rand::rng()) {
                    Ok(avisos) => {
                        if !avisos.is_empty() {
                            difundir(&cadeiras, ParaCliente::Avisos { avisos });
                        }
                        mandar_estado(&cadeiras, &partida);
                    }
                    Err(e) => {
                        // Erro de regra é do jogador que tentou, e só ele ouve. Difundir
                        // "não é a sua vez" para a mesa entregaria informação de quem
                        // tentou o quê.
                        if let Some(c) = cadeiras.get(assento) {
                            let _ = c.canal.send(ParaCliente::Erro {
                                erro: codigo_do_erro(&e),
                                mensagem: e.to_string(),
                            });
                        }
                    }
                }
                if let Some(v) = partida.vencedora {
                    return encerrar(&estado, &ctx, &cadeiras, &partida, v, false).await;
                }
            }
            Comando::Saiu(assento) => {
                if let Some(c) = cadeiras.get_mut(assento) {
                    c.vivo = false;
                }
                // Abandono: a dupla de quem saiu perde. Em 2x2 isso castiga o parceiro, e
                // está declarado nos riscos conhecidos — a alternativa (esperar reconexão)
                // precisa de temporizador e de reentrada, que é outra onda de trabalho.
                let vencedora = 1 - truco_regras::equipe_de(assento) as u8;
                return encerrar(&estado, &ctx, &cadeiras, &partida, vencedora, true).await;
            }
        }
    }
    // Sem `Ok(())` aqui: todo caminho do laço devolve, inclusive o canal fechado. O clippy
    // apontou a expressão inalcançável, e tirá-la é melhor que silenciá-la — ela dizia que
    // existia uma saída da mesa sem liquidação, e não existe.
}

/// Só os jogadores de verdade. Bot não tem webhook nem id: o `filter_map` é o que garante
/// que nenhuma rotina de pagamento ou de entrega o alcance por engano.
fn humanos(cadeiras: &[Cadeira]) -> Vec<String> {
    cadeiras
        .iter()
        .filter_map(|c| c.ocupante.humano().map(|j| j.id.clone()))
        .collect()
}

/// Quem está devendo uma ação agora. É de quem o relógio corre.
fn quem_deve_agir(partida: &Partida) -> usize {
    let m = &partida.mao;
    if m.aguarda_onze() {
        // A decisão é da dupla que está com 11; o relógio corre para o assento dela que
        // puxaria a mão.
        if let truco_regras::TipoMao::Onze { equipe } = m.tipo {
            return (0..partida.assentos())
                .find(|a| truco_regras::equipe_de(*a) == equipe)
                .unwrap_or(m.vez);
        }
    }
    match m.pendencia {
        Some(p) => p.assento_respondente,
        None => m.vez,
    }
}

fn codigo_do_erro(e: &truco_regras::Erro) -> &'static str {
    use truco_regras::Erro as E;
    match e {
        E::PartidaTerminada => "partida_terminada",
        E::NaoEhSuaVez => "nao_eh_sua_vez",
        E::IndiceDeCartaInvalido => "indice_invalido",
        E::CobertaNaPrimeiraRodada => "coberta_na_primeira",
        E::RespondaOPedido => "responda_o_pedido",
        E::NaoHaPedidoParaResponder => "nao_ha_pedido",
        E::ValorNoTeto => "valor_no_teto",
        E::PedidoSeguidoDaMesmaDupla => "pedido_seguido",
        E::NoDozeNaoSeAumenta => "doze_nao_aumenta",
        E::PedidoProibidoNestaMao => "pedido_proibido",
        E::DecidaAMaoDeOnze => "decida_a_onze",
        E::NaoEhMaoDeOnze => "nao_eh_onze",
        E::NaoEhSuaMaoDeOnze => "onze_nao_eh_sua",
    }
}

fn mandar_estado(cadeiras: &[Cadeira], partida: &Partida) {
    for (assento, c) in cadeiras.iter().enumerate() {
        // Visao::para é o que garante que cada socket só recebe o que aquele assento pode
        // ver. O servidor não monta estado de jogo à mão em nenhum lugar.
        let _ = c
            .canal
            .send(ParaCliente::Estado(Visao::para(partida, assento)));
    }
}

fn difundir(cadeiras: &[Cadeira], msg: ParaCliente) {
    for c in cadeiras {
        let _ = c.canal.send(msg.clone());
    }
}

/// Paga, fecha a estatística, avisa a mesa e dispara o webhook de fim.
async fn encerrar(
    estado: &Estado,
    ctx: &Contexto,
    cadeiras: &[Cadeira],
    partida: &Partida,
    vencedora: u8,
    por_abandono: bool,
) -> anyhow::Result<()> {
    let Contexto {
        partida_id,
        modo,
        aposta,
        treino,
    } = ctx;
    let (modo, aposta, treino) = (*modo, *aposta, *treino);
    let placar = partida.placar;
    // A aposta foi debitada de todos na entrada. O bolo é `n * aposta` e vai inteiro para os
    // vencedores, em partes iguais: cada vencedor recebe `2 * aposta` e fica com `+aposta`
    // líquido; cada perdedor fica com `-aposta`.
    let por_vencedor = aposta * 2;
    let lavada = placar[vencedora as usize] >= 12 && placar[1 - vencedora as usize] == 0;

    sqlx::query(
        "UPDATE partidas SET equipe_vencedora = ?2, placar0 = ?3, placar1 = ?4,
         terminou_em = ?5 WHERE id = ?1",
    )
    .bind(partida_id)
    .bind(i64::from(vencedora))
    .bind(i64::from(placar[0]))
    .bind(i64::from(placar[1]))
    .bind(bd::agora())
    .execute(&estado.pool)
    .await?;

    let mut vencedores = Vec::new();
    let mut perdedores = Vec::new();
    for (assento, c) in cadeiras.iter().enumerate() {
        let ganhou = truco_regras::equipe_de(assento) as u8 == vencedora;
        if ganhou {
            vencedores.push(c.ocupante.apelido().to_string());
        } else {
            perdedores.push(c.ocupante.apelido().to_string());
        }
        // Treino não paga e não pontua, e bot não tem ficha para pagar nem pontuar. As duas
        // condições são diferentes e as duas importam: sem a primeira, o ranking vira
        // treino acumulado (que é o risco de farm que esta v1 declarou); sem a segunda,
        // moeda sairia do nada para a mão do bot.
        let Some(jogador) = c.ocupante.humano().filter(|_| !treino) else {
            continue;
        };
        if ganhou {
            bd::creditar(&estado.pool, &jogador.id, por_vencedor).await?;
        }
        bd::registrar_resultado(&estado.pool, &jogador.id, ganhou, lavada).await?;
    }

    for (assento, c) in cadeiras.iter().enumerate() {
        let ganhou = truco_regras::equipe_de(assento) as u8 == vencedora;
        let moedas = match c.ocupante.humano() {
            Some(j) => bd::por_id(&estado.pool, &j.id)
                .await?
                .map_or(0, |j| j.moedas),
            None => 0,
        };
        let _ = c.canal.send(ParaCliente::Fim {
            vencedora,
            placar,
            moedas,
            ganho: if treino {
                0
            } else if ganhou {
                aposta
            } else {
                -aposta
            },
            treino,
        });
    }

    webhooks::disparar(
        estado.pool.clone(),
        estado.cliente.clone(),
        humanos(cadeiras),
        "partida.terminou",
        serde_json::json!({
            "evento": "partida.terminou",
            "em": bd::agora(),
            "partida": {
                "id": partida_id,
                "modo": nome_do_modo(modo),
                "aposta": aposta,
                "treino": treino,
            },
            "resultado": {
                "equipe_vencedora": vencedora,
                "placar": placar,
                "vencedores": vencedores,
                "perdedores": perdedores,
                "moedas_por_vencedor": if treino { 0 } else { por_vencedor },
                "por_abandono": por_abandono,
            }
        }),
    );

    tracing::info!(
        partida = partida_id,
        vencedora,
        por_abandono,
        "mesa encerrada"
    );
    Ok(())
}
