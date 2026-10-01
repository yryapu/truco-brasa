//! Cadastro, sessão e isolamento (ADR-002).
//!
//! Dois hashes, com propósitos diferentes e parâmetros diferentes de propósito:
//! - **senha**: Argon2id. É entrada humana, de baixa entropia, e precisa de custo.
//! - **token de sessão**: SHA-256 cru. São 32 bytes uniformes do CSPRNG — não existe
//!   dicionário a atacar, e pôr Argon2 aqui adicionaria o seu custo a **toda** requisição
//!   autenticada em troca de nada.

// password-hash 0.6: `hash_password` gera o sal grande e aleatório por conta própria
// (feature `getrandom`, que o default do argon2 liga), e `PasswordHash` passou a viver em
// `password_hash::phc`. Nas versões anteriores era `SaltString::generate(&mut OsRng)` à mão.
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, request::Parts};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::bd::{self, Jogador};
use crate::estado::Estado;
use crate::falha::{Falha, R};

pub const COOKIE_SESSAO: &str = "sessao";

pub fn hash_de_senha(senha: &str) -> anyhow::Result<String> {
    Ok(Argon2::default()
        .hash_password(senha.as_bytes())
        .map_err(|e| anyhow::anyhow!("argon2: {e}"))?
        .to_string())
}

pub fn senha_confere(senha: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else { return false };
    Argon2::default().verify_password(senha.as_bytes(), &parsed).is_ok()
}

/// Token novo: 32 bytes do CSPRNG em hexadecimal. Volta `(token, hash do token)` — o token
/// vai para o cookie e **nunca** para o banco.
pub fn novo_token() -> (String, String) {
    let bytes: [u8; 32] = rand::random();
    let token = base16ct::lower::encode_string(&bytes);
    let hash = hash_de_token(&token);
    (token, hash)
}

pub fn hash_de_token(token: &str) -> String {
    base16ct::lower::encode_string(&Sha256::digest(token.as_bytes()))
}

pub async fn criar_sessao(pool: &SqlitePool, jogador_id: &str) -> R<String> {
    let (token, hash) = novo_token();
    sqlx::query("INSERT INTO sessoes (token_hash, jogador_id, criado_em) VALUES (?1, ?2, ?3)")
        .bind(&hash)
        .bind(jogador_id)
        .bind(bd::agora())
        .execute(pool)
        .await?;
    Ok(token)
}

pub async fn encerrar_sessao(pool: &SqlitePool, token: &str) -> R<()> {
    sqlx::query("DELETE FROM sessoes WHERE token_hash = ?1")
        .bind(hash_de_token(token))
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn jogador_da_sessao(pool: &SqlitePool, token: &str) -> R<Option<Jogador>> {
    let linha: Option<(String,)> =
        sqlx::query_as("SELECT jogador_id FROM sessoes WHERE token_hash = ?1")
            .bind(hash_de_token(token))
            .fetch_optional(pool)
            .await?;
    match linha {
        None => Ok(None),
        Some((id,)) => Ok(bd::por_id(pool, &id).await?),
    }
}

/// Lê o cookie de sessão de um `Cookie:` cru. Sem crate de cookie: é um cabeçalho de pares
/// `nome=valor` separados por `;`, e o nosso valor é hexadecimal.
pub fn token_do_cabecalho(headers: &HeaderMap) -> Option<String> {
    let bruto = headers.get(COOKIE)?.to_str().ok()?;
    bruto
        .split(';')
        .map(str::trim)
        .filter_map(|par| par.split_once('='))
        .find(|(n, _)| *n == COOKIE_SESSAO)
        .map(|(_, v)| v.to_string())
}

/// `Secure` só quando servido por HTTPS: num `http://127.0.0.1` de desenvolvimento, um cookie
/// `Secure` simplesmente não é guardado pelo navegador, e o jogo não entraria.
pub fn cookie_de_sessao(token: &str, seguro: bool) -> HeaderValue {
    let mut v = format!(
        "{COOKIE_SESSAO}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000"
    );
    if seguro {
        v.push_str("; Secure");
    }
    HeaderValue::from_str(&v).expect("token é hexadecimal, cabe num cabeçalho")
}

pub fn cookie_apagado(seguro: bool) -> HeaderValue {
    let mut v = format!("{COOKIE_SESSAO}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
    if seguro {
        v.push_str("; Secure");
    }
    HeaderValue::from_str(&v).expect("constante")
}

pub fn com_cookie(token: &str, seguro: bool) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(SET_COOKIE, cookie_de_sessao(token, seguro));
    h
}

/// Extrator: **a identidade vem daqui e de nenhum outro lugar**. Nenhuma rota aceita
/// `jogador_id` do cliente (ADR-002) — é este tipo que torna isso estrutural em vez de
/// disciplina, porque uma rota que quer saber quem é só tem este caminho.
pub struct Autenticado(pub Jogador);

impl FromRequestParts<Estado> for Autenticado {
    type Rejection = Falha;

    async fn from_request_parts(parts: &mut Parts, estado: &Estado) -> Result<Self, Falha> {
        let token = token_do_cabecalho(&parts.headers).ok_or(Falha::NaoAutenticado)?;
        let jogador =
            jogador_da_sessao(&estado.pool, &token).await?.ok_or(Falha::NaoAutenticado)?;
        Ok(Autenticado(jogador))
    }
}
