//! O erro que sai pela API. Um tipo só, e ele sabe o seu status HTTP.
//!
//! O cliente recebe sempre `{"erro":"<código>","mensagem":"<português>"}`: o código é para
//! o programa, a mensagem é para a pessoa. Nenhuma variante vaza detalhe interno — erro de
//! banco vira `interno` e vai para o log, não para a resposta.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum Falha {
    #[error("apelido já está em uso")]
    ApelidoEmUso,
    #[error("apelido ou senha não conferem")]
    CredenciaisInvalidas,
    #[error("entre para fazer isso")]
    NaoAutenticado,
    #[error("o apelido precisa de 3 a 20 caracteres, sem espaço")]
    ApelidoInvalido,
    #[error("a senha precisa de pelo menos 8 caracteres")]
    SenhaCurta,
    #[error("isso não é uma URL https válida")]
    UrlInvalida,
    /// SSRF: o destino aponta para dentro da nossa própria rede (ADR-004).
    #[error("esse destino não é permitido")]
    DestinoProibido,
    #[error("você já tem webhooks demais")]
    LimiteDeWebhooks,
    #[error("não achei isso")]
    NaoAchei,
    #[error("aposta inválida para o seu saldo")]
    ApostaInvalida,
    #[error("modo de jogo desconhecido")]
    ModoInvalido,
    #[error("erro interno")]
    Interno(#[from] anyhow::Error),
}

impl Falha {
    fn codigo(&self) -> &'static str {
        match self {
            Falha::ApelidoEmUso => "apelido_em_uso",
            Falha::CredenciaisInvalidas => "credenciais_invalidas",
            Falha::NaoAutenticado => "nao_autenticado",
            Falha::ApelidoInvalido => "apelido_invalido",
            Falha::SenhaCurta => "senha_curta",
            Falha::UrlInvalida => "url_invalida",
            Falha::DestinoProibido => "destino_proibido",
            Falha::LimiteDeWebhooks => "limite_de_webhooks",
            Falha::NaoAchei => "nao_achei",
            Falha::ApostaInvalida => "aposta_invalida",
            Falha::ModoInvalido => "modo_invalido",
            Falha::Interno(_) => "interno",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Falha::ApelidoEmUso => StatusCode::CONFLICT,
            Falha::CredenciaisInvalidas | Falha::NaoAutenticado => StatusCode::UNAUTHORIZED,
            Falha::NaoAchei => StatusCode::NOT_FOUND,
            Falha::Interno(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }
}

impl From<sqlx::Error> for Falha {
    fn from(e: sqlx::Error) -> Self {
        Falha::Interno(e.into())
    }
}

impl IntoResponse for Falha {
    fn into_response(self) -> Response {
        if let Falha::Interno(ref e) = self {
            // O detalhe fica no log do servidor. O cliente recebe "erro interno".
            tracing::error!(erro = %e, "falha interna");
        }
        let corpo = serde_json::json!({ "erro": self.codigo(), "mensagem": self.to_string() });
        (self.status(), axum::Json(corpo)).into_response()
    }
}

pub type R<T> = Result<T, Falha>;
