//! As rotas HTTP. Nenhuma delas aceita identidade do cliente: quem pergunta vem do cookie.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header::SET_COOKIE};
use axum::{Json, response::IntoResponse};
use serde::{Deserialize, Serialize};

use crate::auth::{self, Autenticado};
use crate::bd::{self, Jogador};
use crate::emblemas::{self, Emblema};
use crate::estado::Estado;
use crate::falha::{Falha, R};
use crate::webhooks::{self, Webhook};

#[derive(Debug, Deserialize)]
pub struct Credenciais {
    pub apelido: String,
    pub senha: String,
}

#[derive(Debug, Serialize)]
pub struct Ficha {
    #[serde(flatten)]
    pub jogador: Jogador,
    pub emblemas: Vec<Emblema>,
}

impl From<Jogador> for Ficha {
    fn from(j: Jogador) -> Ficha {
        let emblemas = emblemas::de(&j);
        Ficha {
            jogador: j,
            emblemas,
        }
    }
}

/// 3 a 20 caracteres, sem espaço nem controle. Curto para ser rápido de digitar; sem espaço
/// para que dois apelidos não se confundam visualmente na mesa.
fn apelido_valido(a: &str) -> bool {
    let n = a.chars().count();
    (3..=20).contains(&n) && a.chars().all(|c| !c.is_whitespace() && !c.is_control())
}

pub async fn registrar(
    State(estado): State<Estado>,
    Json(c): Json<Credenciais>,
) -> R<impl IntoResponse> {
    let apelido = c.apelido.trim();
    if !apelido_valido(apelido) {
        return Err(Falha::ApelidoInvalido);
    }
    if c.senha.chars().count() < 8 {
        return Err(Falha::SenhaCurta);
    }
    let hash = auth::hash_de_senha(&c.senha).map_err(Falha::Interno)?;
    let jogador = match bd::criar_jogador(&estado.pool, apelido, &hash).await {
        Ok(j) => j,
        // O índice único é quem decide: conferir antes e inserir depois deixaria uma janela
        // para dois registros simultâneos do mesmo apelido.
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(Falha::ApelidoEmUso);
        }
        Err(e) => return Err(e.into()),
    };
    // Registrar já entra: não existe "cadastrou, agora faça login" (ADR-002).
    let token = auth::criar_sessao(&estado.pool, &jogador.id).await?;
    Ok((
        auth::com_cookie(&token, estado.config.cookie_seguro),
        Json(Ficha::from(jogador)),
    ))
}

pub async fn entrar(
    State(estado): State<Estado>,
    Json(c): Json<Credenciais>,
) -> R<impl IntoResponse> {
    let Some((jogador, hash)) = bd::por_apelido(&estado.pool, c.apelido.trim()).await? else {
        return Err(Falha::CredenciaisInvalidas);
    };
    if !auth::senha_confere(&c.senha, &hash) {
        return Err(Falha::CredenciaisInvalidas);
    }
    let token = auth::criar_sessao(&estado.pool, &jogador.id).await?;
    Ok((
        auth::com_cookie(&token, estado.config.cookie_seguro),
        Json(Ficha::from(jogador)),
    ))
}

pub async fn sair(State(estado): State<Estado>, headers: HeaderMap) -> R<impl IntoResponse> {
    if let Some(token) = auth::token_do_cabecalho(&headers) {
        auth::encerrar_sessao(&estado.pool, &token).await?;
    }
    let mut h = HeaderMap::new();
    h.insert(
        SET_COOKIE,
        auth::cookie_apagado(estado.config.cookie_seguro),
    );
    Ok((StatusCode::NO_CONTENT, h))
}

pub async fn eu(Autenticado(jogador): Autenticado) -> Json<Ficha> {
    Json(Ficha::from(jogador))
}

/// A linha do ranking. **Sem o `id`**: o ranking é público, e o id interno do jogador não
/// tem nada a fazer numa rota sem autenticação — ele só serve para o servidor. Achado ao
/// olhar a saída de `GET /api/ranking` no teste de fumaça, não por revisão de código.
#[derive(Debug, Serialize)]
pub struct LinhaDoRanking {
    pub apelido: String,
    pub moedas: i64,
    pub partidas: i64,
    pub vitorias: i64,
    pub derrotas: i64,
    pub melhor_sequencia: i64,
    pub emblemas: Vec<Emblema>,
}

pub async fn ranking(State(estado): State<Estado>) -> R<Json<Vec<LinhaDoRanking>>> {
    let lista = bd::ranking(&estado.pool, 50).await?;
    Ok(Json(
        lista
            .into_iter()
            .map(|j| LinhaDoRanking {
                emblemas: emblemas::de(&j),
                apelido: j.apelido,
                moedas: j.moedas,
                partidas: j.partidas,
                vitorias: j.vitorias,
                derrotas: j.derrotas,
                melhor_sequencia: j.melhor_sequencia,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct NovoWebhook {
    pub url: String,
}

pub async fn registrar_webhook(
    State(estado): State<Estado>,
    Autenticado(jogador): Autenticado,
    Json(n): Json<NovoWebhook>,
) -> R<Json<Webhook>> {
    let w = webhooks::registrar(
        &estado.pool,
        &jogador.id,
        n.url.trim(),
        estado.config.permitir_http_webhook,
        estado.config.permitir_destino_privado,
    )
    .await?;
    Ok(Json(w))
}

pub async fn listar_webhooks(
    State(estado): State<Estado>,
    Autenticado(jogador): Autenticado,
) -> R<Json<Vec<Webhook>>> {
    Ok(Json(webhooks::listar(&estado.pool, &jogador.id).await?))
}

pub async fn remover_webhook(
    State(estado): State<Estado>,
    Autenticado(jogador): Autenticado,
    Path(id): Path<String>,
) -> R<StatusCode> {
    webhooks::remover(&estado.pool, &jogador.id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// O cliente vai **dentro do binário**. Um arquivo a servir é um arquivo a montar no
/// container, a achar por caminho relativo e a errar em produção; embutido, `cargo run` e
/// `docker run` servem exatamente o mesmo cliente que o teste exercitou.
pub async fn indice() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../../../cliente/index.html"),
    )
}

pub async fn saude() -> &'static str {
    "ok"
}
