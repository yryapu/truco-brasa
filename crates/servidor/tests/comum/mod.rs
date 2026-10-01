//! Ajudas compartilhadas pelos testes de integração: servidor de verdade numa porta
//! efêmera, cliente HTTP+WebSocket, e um jogador automático que termina a partida.
//!
//! Cada arquivo de teste compila este módulo inteiro e nenhum usa tudo.
#![allow(dead_code)]

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Message, http::HeaderValue};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use truco_servidor::estado::Config;

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Sobe um servidor de verdade numa porta efêmera. Devolve o endereço e o caminho do banco.
pub async fn servidor(permitir_webhook_local: bool) -> (SocketAddr, tempdir::Guarda) {
    let dir = tempdir::criar();
    let banco = format!("sqlite://{}/truco.db", dir.caminho());
    let app = truco_servidor::montar(
        &banco,
        Config {
            cookie_seguro: false,
            permitir_http_webhook: true,
            permitir_destino_privado: permitir_webhook_local,
        },
    )
    .await
    .expect("servidor montou");
    let escuta = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("porta efêmera");
    let endereco = escuta.local_addr().expect("endereço");
    tokio::spawn(async move {
        let _ = axum::serve(escuta, app).await;
    });
    (endereco, dir)
}

/// Diretório temporário sem crate externa: três linhas, e o teste não ganha dependência.
mod tempdir {
    pub struct Guarda(std::path::PathBuf);
    impl Guarda {
        pub fn caminho(&self) -> String {
            self.0.display().to_string()
        }
    }
    impl Drop for Guarda {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    pub fn criar() -> Guarda {
        let p = std::env::temp_dir().join(format!("truco-teste-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).expect("criou diretório temporário");
        Guarda(p)
    }
}

pub struct Cliente {
    pub http: reqwest::Client,
    pub base: String,
    pub token: String,
    pub apelido: String,
}

impl Cliente {
    /// Registra e já sai logado — é o que o ADR-002 promete.
    pub async fn registrar(endereco: SocketAddr, apelido: &str) -> Cliente {
        let base = format!("http://{endereco}");
        let http = reqwest::Client::new();
        let r = http
            .post(format!("{base}/api/registrar"))
            .json(&json!({"apelido": apelido, "senha": "senha-de-teste-1"}))
            .send()
            .await
            .expect("registrou");
        assert_eq!(r.status(), 200, "registro devia dar 200");
        let cookie = r
            .headers()
            .get("set-cookie")
            .expect("o registro devolve a sessão")
            .to_str()
            .unwrap()
            .to_string();
        let token = cookie
            .split(';')
            .next()
            .and_then(|p| p.split_once('='))
            .map(|(_, v)| v.to_string())
            .expect("cookie sessao=...");
        let ficha: Value = r.json().await.expect("ficha");
        assert_eq!(ficha["moedas"], 1000, "todo jogador começa com 1000 moedas");
        Cliente {
            http,
            base,
            token,
            apelido: apelido.to_string(),
        }
    }

    pub async fn eu(&self) -> Value {
        self.http
            .get(format!("{}/api/eu", self.base))
            .header("Cookie", format!("sessao={}", self.token))
            .send()
            .await
            .expect("eu")
            .json()
            .await
            .expect("json")
    }

    pub async fn moedas(&self) -> i64 {
        self.eu().await["moedas"].as_i64().expect("moedas")
    }

    pub async fn conectar(&self, modo: &str, aposta: i64) -> Socket {
        let url = format!(
            "ws://{}/ws?modo={modo}&aposta={aposta}",
            self.base.trim_start_matches("http://")
        );
        let mut req = url.into_client_request().expect("url de ws");
        req.headers_mut().insert(
            "Cookie",
            HeaderValue::from_str(&format!("sessao={}", self.token)).unwrap(),
        );
        let (s, _) = tokio_tungstenite::connect_async(req)
            .await
            .expect("conectou o ws");
        s
    }
}

pub async fn proxima(s: &mut Socket) -> Value {
    loop {
        match s.next().await {
            Some(Ok(Message::Text(t))) => {
                return serde_json::from_str(&t).expect("o servidor manda JSON");
            }
            Some(Ok(_)) => continue,
            outro => panic!("socket morreu esperando mensagem: {outro:?}"),
        }
    }
}

pub async fn manda(s: &mut Socket, v: Value) {
    s.send(Message::Text(v.to_string().into()))
        .await
        .expect("mandou");
}

/// Joga de forma simples e **terminante**: aceita todo truco, nunca aumenta, e sempre põe a
/// primeira carta. O objetivo é provar que uma partida inteira fecha, não jogar bem.
/// Devolve a mensagem `fim`.
pub async fn jogar_ate_o_fim(mut s: Socket, rotulo: &str) -> Value {
    let mut passos = 0;
    loop {
        passos += 1;
        assert!(
            passos < 4000,
            "{rotulo}: partida não terminou em 4000 mensagens"
        );
        let m = proxima(&mut s).await;
        match m["t"].as_str().unwrap_or_default() {
            "fim" => return m,
            "estado" => {
                let acoes: Vec<&str> = m["acoes"]
                    .as_array()
                    .map_or(vec![], |a| a.iter().filter_map(Value::as_str).collect());
                // Invariante do protocolo: tudo em `minhas_cartas` é carta do truco.
                for c in m["minhas_cartas"].as_array().expect("minhas_cartas") {
                    let s = c.as_str().expect("a carta é uma string de um caractere");
                    let mut cs = s.chars();
                    let ch = cs.next().expect("um caractere");
                    assert!(cs.next().is_none(), "a carta é UM caractere, veio {s:?}");
                    assert!(
                        truco_regras::Carta::do_unicode(ch).is_some(),
                        "{ch:?} não é carta do baralho de 40"
                    );
                }
                if acoes.contains(&"onze_aceitar") {
                    manda(&mut s, json!({"t":"onze","aceita":true})).await;
                } else if acoes.contains(&"aceitar") {
                    manda(&mut s, json!({"t":"aceitar"})).await;
                } else if acoes.contains(&"jogar") {
                    manda(&mut s, json!({"t":"jogar","indice":0,"coberta":false})).await;
                }
            }
            // `fila`, `mesa`, `avisos` e `erro` não mudam o que este jogador faz.
            _ => {}
        }
    }
}

impl Cliente {
    fn com_sessao(&self, r: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        r.header("Cookie", format!("sessao={}", self.token))
    }
    pub async fn post(&self, rota: &str, corpo: Value) -> reqwest::Response {
        self.com_sessao(self.http.post(format!("{}{rota}", self.base)).json(&corpo))
            .send()
            .await
            .expect("post")
    }
    pub async fn get(&self, rota: &str) -> reqwest::Response {
        self.com_sessao(self.http.get(format!("{}{rota}", self.base)))
            .send()
            .await
            .expect("get")
    }
    pub async fn delete(&self, rota: &str) -> reqwest::Response {
        self.com_sessao(self.http.delete(format!("{}{rota}", self.base)))
            .send()
            .await
            .expect("delete")
    }
}
