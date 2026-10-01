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
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct NaMesa {
    pub assento: usize,
    pub apelido: String,
    pub equipe: u8,
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
}

struct Cadeira {
    jogador: Jogador,
    canal: mpsc::UnboundedSender<ParaCliente>,
    /// O socket já foi embora? Mesa não morre por causa de um `send` para quem saiu.
    vivo: bool,
}

/// Monta a mesa e roda a partida até o fim. Consome as esperas.
pub fn abrir(estado: Estado, modo: Modo, aposta: i64, esperas: Vec<Espera>) {
    tokio::spawn(async move {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let mut cadeiras = Vec::new();
        for (assento, e) in esperas.into_iter().enumerate() {
            // Se o socket morreu entre entrar na fila e a mesa abrir, este envio falha — e
            // aí a mesa começa com um assento morto, que o laço trata como abandono.
            let vivo = e.entrou_na_mesa.send((assento, cmd_tx.clone())).is_ok();
            cadeiras.push(Cadeira {
                jogador: e.jogador,
                canal: e.para_cliente,
                vivo,
            });
        }
        if let Err(e) = rodar(estado, modo, aposta, cadeiras, cmd_rx).await {
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
) -> anyhow::Result<()> {
    let partida_id = uuid::Uuid::now_v7().to_string();
    let na_mesa: Vec<NaMesa> = cadeiras
        .iter()
        .enumerate()
        .map(|(assento, c)| NaMesa {
            assento,
            apelido: c.jogador.apelido.clone(),
            equipe: truco_regras::equipe_de(assento),
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
    };
    let mut partida = Partida::nova(modo, &mut rand::rng());

    for (assento, c) in cadeiras.iter().enumerate() {
        let _ = c.canal.send(ParaCliente::Mesa {
            partida: partida_id.clone(),
            assento,
            aposta,
            jogadores: na_mesa.clone(),
        });
    }
    mandar_estado(&cadeiras, &partida);

    webhooks::disparar(
        estado.pool.clone(),
        estado.cliente.clone(),
        cadeiras.iter().map(|c| c.jogador.id.clone()).collect(),
        "partida.comecou",
        serde_json::json!({
            "evento": "partida.comecou",
            "em": bd::agora(),
            "partida": {
                "id": partida_id,
                "modo": nome_do_modo(modo),
                "aposta": aposta,
                "jogadores": na_mesa,
            }
        }),
    );

    // Um assento que já nasceu morto encerra a partida antes do primeiro comando.
    if let Some(morto) = cadeiras.iter().position(|c| !c.vivo) {
        let vencedora = 1 - truco_regras::equipe_de(morto) as u8;
        return encerrar(&estado, &ctx, &cadeiras, &partida, vencedora, true).await;
    }

    while let Some(cmd) = cmd_rx.recv().await {
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
    Ok(())
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
    } = ctx;
    let (modo, aposta) = (*modo, *aposta);
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
            bd::creditar(&estado.pool, &c.jogador.id, por_vencedor).await?;
            vencedores.push(c.jogador.apelido.clone());
        } else {
            perdedores.push(c.jogador.apelido.clone());
        }
        bd::registrar_resultado(&estado.pool, &c.jogador.id, ganhou, lavada).await?;
    }

    for (assento, c) in cadeiras.iter().enumerate() {
        let ganhou = truco_regras::equipe_de(assento) as u8 == vencedora;
        let moedas = bd::por_id(&estado.pool, &c.jogador.id)
            .await?
            .map_or(0, |j| j.moedas);
        let _ = c.canal.send(ParaCliente::Fim {
            vencedora,
            placar,
            moedas,
            ganho: if ganhou { aposta } else { -aposta },
        });
    }

    webhooks::disparar(
        estado.pool.clone(),
        estado.cliente.clone(),
        cadeiras.iter().map(|c| c.jogador.id.clone()).collect(),
        "partida.terminou",
        serde_json::json!({
            "evento": "partida.terminou",
            "em": bd::agora(),
            "partida": { "id": partida_id, "modo": nome_do_modo(modo), "aposta": aposta },
            "resultado": {
                "equipe_vencedora": vencedora,
                "placar": placar,
                "vencedores": vencedores,
                "perdedores": perdedores,
                "moedas_por_vencedor": por_vencedor,
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
