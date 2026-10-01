//! O WebSocket: autentica pelo mesmo cookie, entra na fila, e vira um par de laços.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use truco_regras::Modo;

use crate::auth::Autenticado;
use crate::bd;
use crate::estado::{Chave, Espera, Estado};
use crate::falha::{Falha, R};
use crate::mesas::{self, Comando, DoCliente, ParaCliente};

#[derive(Debug, serde::Deserialize)]
pub struct Parametros {
    pub modo: String,
    #[serde(default)]
    pub aposta: i64,
}

pub async fn entrar(
    ws: WebSocketUpgrade,
    Query(p): Query<Parametros>,
    Autenticado(jogador): Autenticado,
    State(estado): State<Estado>,
) -> R<Response> {
    let modo = match p.modo.as_str() {
        "1x1" => Modo::UmContraUm,
        "2x2" => Modo::DoisContraDois,
        _ => return Err(Falha::ModoInvalido),
    };
    if p.aposta < 0 || p.aposta > jogador.moedas {
        return Err(Falha::ApostaInvalida);
    }
    // Debita **antes** de entrar na fila, e a condição de saldo está no próprio UPDATE: não
    // existe janela entre conferir e cobrar, nem com duas abas do mesmo jogador.
    if !bd::debitar(&estado.pool, &jogador.id, p.aposta).await? {
        return Err(Falha::ApostaInvalida);
    }

    let aposta = p.aposta;
    Ok(ws.on_upgrade(move |socket| conduzir(socket, estado, jogador, modo, aposta)))
}

async fn conduzir(
    socket: WebSocket,
    estado: Estado,
    jogador: bd::Jogador,
    modo: Modo,
    aposta: i64,
) {
    let chave: Chave = (modo, aposta);
    let (mut escritor, mut leitor) = socket.split();
    let (para_cliente, mut saida) = mpsc::unbounded_channel::<ParaCliente>();

    // Uma tarefa só escreve no socket. Tudo que o servidor quer dizer passa pelo canal.
    let escrevendo = tokio::spawn(async move {
        while let Some(msg) = saida.recv().await {
            let texto = match serde_json::to_string(&msg) {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!(erro = %e, "mensagem não serializou");
                    continue;
                }
            };
            if escritor.send(Message::Text(texto.into())).await.is_err() {
                break;
            }
        }
        let _ = escritor.close().await;
    });

    // Uma tarefa só lê do socket e repassa cru. Com isso o fluxo principal pode esperar
    // "a mesa abriu" e "o socket caiu" ao mesmo tempo, sem consumir o receptor da mesa.
    let (bruto_tx, mut bruto) = mpsc::unbounded_channel();
    let lendo = tokio::spawn(async move {
        while let Some(m) = leitor.next().await {
            if bruto_tx.send(m).is_err() {
                break;
            }
        }
    });

    let (entrou_tx, mut entrou_rx) = oneshot::channel();
    let id = uuid::Uuid::new_v4();
    let espera = Espera {
        id,
        jogador: jogador.clone(),
        para_cliente: para_cliente.clone(),
        entrou_na_mesa: entrou_tx,
    };
    let pronta = estado.enfileirar(chave, espera);
    let faltam = modo.assentos().saturating_sub(estado.quantos_esperando(chave));
    let _ = para_cliente.send(ParaCliente::Fila {
        modo: mesas::nome_do_modo(modo),
        aposta,
        faltam,
    });
    if let Some(esperas) = pronta {
        mesas::abrir(estado.clone(), modo, aposta, esperas);
    }

    // Enquanto espera mesa, o socket ainda pode cair. Sem isto, quem fecha a aba na fila
    // ficaria lá segurando um assento e a aposta dele.
    let (assento, comandos) = loop {
        tokio::select! {
            r = &mut entrou_rx => match r {
                Ok(v) => break v,
                Err(_) => {
                    devolver_se_desistiu(&estado, chave, id, &jogador.id, aposta).await;
                    escrevendo.abort();
                    lendo.abort();
                    return;
                }
            },
            m = bruto.recv() => match m {
                // Socket fechado ou com erro antes de haver mesa: desistência.
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => {
                    devolver_se_desistiu(&estado, chave, id, &jogador.id, aposta).await;
                    escrevendo.abort();
                    lendo.abort();
                    return;
                }
                // Mensagem solta antes da mesa: nada a fazer com ela.
                Some(Ok(_)) => continue,
            },
        }
    };

    // Daqui para baixo existe mesa: cada mensagem válida é um comando.
    while let Some(Ok(msg)) = bruto.recv().await {
        let texto = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            // Ping/Pong são tratados pelo axum; binário não faz parte do protocolo.
            _ => continue,
        };
        match serde_json::from_str::<DoCliente>(&texto) {
            Ok(m) => {
                if comandos.send(Comando::Agir(assento, m.into())).is_err() {
                    break;
                }
            }
            Err(e) => {
                let _ = para_cliente.send(ParaCliente::Erro {
                    erro: "mensagem_invalida",
                    mensagem: format!("não entendi essa mensagem: {e}"),
                });
            }
        }
    }

    let _ = comandos.send(Comando::Saiu(assento));
    escrevendo.abort();
    lendo.abort();
}



/// Devolve a aposta **se** este jogador ainda estava na fila. A condição importa: se a mesa
/// já tinha levado a espera, a aposta está em jogo e devolver aqui seria criar moeda.
async fn devolver_se_desistiu(
    estado: &Estado,
    chave: Chave,
    id: uuid::Uuid,
    jogador_id: &str,
    aposta: i64,
) {
    if estado.desistir(chave, id) {
        if let Err(e) = bd::creditar(&estado.pool, jogador_id, aposta).await {
            tracing::error!(erro = %e, "não consegui devolver a aposta de quem saiu da fila");
        }
    }
}
