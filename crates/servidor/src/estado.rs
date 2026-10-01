//! Estado compartilhado do servidor, e a fila de pareamento.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};
use truco_regras::Modo;

use crate::bd::Jogador;
use crate::mesas::{Comando, ParaCliente};

#[derive(Debug, Clone)]
pub struct Config {
    /// Marca o cookie de sessão como `Secure`. Falso em `http://127.0.0.1`, senão o
    /// navegador descarta o cookie e ninguém entra.
    pub cookie_seguro: bool,
    /// Permite registrar webhook `http://`. Só para desenvolvimento e teste (ADR-004).
    pub permitir_http_webhook: bool,
    /// **Desliga a guarda de destino** e deixa o webhook apontar para endereço privado ou
    /// de laço. Existe por um motivo só: sem isso não há como testar entrega de webhook
    /// localmente, porque o receptor de teste mora em `127.0.0.1`. Padrão `false` mesmo em
    /// desenvolvimento, e só liga por `TRUCO_WEBHOOK_LOCAL=1` — nunca junto de
    /// `TRUCO_SEGURO=1`, e o `main` recusa a combinação.
    pub permitir_destino_privado: bool,
    /// Quanto tempo a mesa espera por uma ação de quem está devendo jogada.
    ///
    /// Existe porque "o jogador cai" e "o jogador **para**" são coisas diferentes: queda
    /// fecha o socket e a mesa trata como abandono, mas quem deixa a aba aberta e não age
    /// nunca fecha nada — e sem prazo a mesa fica de pé para sempre com a aposta do
    /// adversário presa dentro dela.
    pub prazo_de_jogada: std::time::Duration,
}

/// Um jogador esperando mesa.
pub struct Espera {
    pub id: uuid::Uuid,
    pub jogador: Jogador,
    pub para_cliente: mpsc::UnboundedSender<ParaCliente>,
    /// Por onde a mesa, quando nascer, avisa este socket como falar com ela.
    pub entrou_na_mesa: oneshot::Sender<(usize, mpsc::UnboundedSender<Comando>)>,
}

/// Chave de pareamento: mesmo modo **e** mesma aposta (ADR-007).
pub type Chave = (Modo, i64);

#[derive(Clone)]
pub struct Estado {
    pub pool: sqlx::SqlitePool,
    pub cliente: reqwest::Client,
    pub config: Config,
    fila: Arc<Mutex<HashMap<Chave, Vec<Espera>>>>,
}

impl Estado {
    pub fn novo(pool: sqlx::SqlitePool, config: Config) -> Estado {
        let cliente = reqwest::Client::builder()
            // Sem seguir redirecionamento: redirect é como se escapa de uma guarda de
            // destino — valida-se o host registrado, e o 302 leva para outro (ADR-004).
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("truco-brasa/0.1")
            .build()
            .expect("cliente HTTP com configuração constante");
        Estado {
            pool,
            cliente,
            config,
            fila: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Põe na fila. Se completou mesa, devolve os jogadores dela — e **tira todos da fila**
    /// na mesma seção crítica, que é o que impede dois pareamentos com o mesmo jogador.
    pub fn enfileirar(&self, chave: Chave, quem: Espera) -> Option<Vec<Espera>> {
        let precisa = chave.0.assentos();
        let mut fila = self.fila.lock().expect("fila envenenada");
        let banco = fila.entry(chave).or_default();
        banco.push(quem);
        if banco.len() >= precisa {
            Some(banco.drain(..precisa).collect())
        } else {
            None
        }
    }

    pub fn quantos_esperando(&self, chave: Chave) -> usize {
        self.fila
            .lock()
            .expect("fila envenenada")
            .get(&chave)
            .map_or(0, Vec::len)
    }

    /// Sai da fila sem ter jogado — o jogador fechou a aba enquanto esperava.
    /// Devolve `true` se ainda estava lá (e portanto é quem deve ser reembolsado).
    pub fn desistir(&self, chave: Chave, id: uuid::Uuid) -> bool {
        let mut fila = self.fila.lock().expect("fila envenenada");
        let Some(banco) = fila.get_mut(&chave) else {
            return false;
        };
        match banco.iter().position(|e| e.id == id) {
            Some(i) => {
                banco.remove(i);
                true
            }
            None => false,
        }
    }
}
