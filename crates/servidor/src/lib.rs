//! Servidor do truco paulista.
//!
//! `lib.rs` monta o roteador e `main.rs` só decide porta e banco — assim o teste de
//! integração sobe o servidor de verdade, no mesmo código, sem subprocesso.

pub mod api;
pub mod auth;
pub mod bd;
pub mod emblemas;
pub mod estado;
pub mod falha;
pub mod mesas;
pub mod webhooks;
pub mod ws;

use axum::Router;
use axum::routing::{delete, get, post};
use estado::{Config, Estado};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

pub fn roteador(estado: Estado) -> Router {
    Router::new()
        .route("/", get(api::indice))
        .route("/saude", get(api::saude))
        .route("/api/registrar", post(api::registrar))
        .route("/api/entrar", post(api::entrar))
        .route("/api/sair", post(api::sair))
        .route("/api/eu", get(api::eu))
        .route("/api/ranking", get(api::ranking))
        .route(
            "/api/webhooks",
            post(api::registrar_webhook).get(api::listar_webhooks),
        )
        // axum 0.8 usa `{id}`, não `:id`.
        .route("/api/webhooks/{id}", delete(api::remover_webhook))
        .route("/ws", get(ws::entrar))
        // Nenhum corpo legítimo desta API passa de 8 KiB. O teto evita que um POST grande
        // vire trabalho de desserialização de graça.
        .layer(RequestBodyLimitLayer::new(8 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(estado)
}

pub async fn montar(url_do_banco: &str, config: Config) -> anyhow::Result<Router> {
    let pool = bd::abrir(url_do_banco).await?;
    Ok(roteador(Estado::novo(pool, config)))
}
