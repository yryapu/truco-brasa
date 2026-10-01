//! Webhooks: registro com guarda de destino, e entrega assinada (ADR-004).

use std::net::IpAddr;
use std::time::Duration;

// hmac 0.13: `new_from_slice` vem do trait `KeyInit`, que precisa estar no escopo.
use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use sqlx::SqlitePool;

use crate::bd;
use crate::falha::{Falha, R};

pub const LIMITE_POR_JOGADOR: i64 = 5;
const TEMPO_LIMITE: Duration = Duration::from_secs(5);
const ESPERA_ANTES_DE_REPETIR: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, serde::Serialize)]
pub struct Webhook {
    pub id: String,
    pub url: String,
    pub criado_em: String,
    /// Só vai preenchido na resposta do registro, uma vez. Nas listagens é `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segredo: Option<String>,
}

/// Valida a URL **e o destino**. Um endpoint que faz o servidor buscar URL arbitrária é
/// SSRF por desenho: sem esta função, um jogador registra `http://169.254.169.254/...` e usa
/// o nosso servidor como procurador contra a nossa própria rede.
pub async fn validar_destino(url: &str, permitir_http: bool, permitir_privado: bool) -> R<()> {
    let u = reqwest::Url::parse(url).map_err(|_| Falha::UrlInvalida)?;
    match u.scheme() {
        "https" => {}
        "http" if permitir_http => {}
        _ => return Err(Falha::UrlInvalida),
    }
    let host = u.host_str().ok_or(Falha::UrlInvalida)?;
    let porta = u.port_or_known_default().ok_or(Falha::UrlInvalida)?;

    let enderecos = tokio::net::lookup_host((host, porta))
        .await
        .map_err(|_| Falha::DestinoProibido)?
        .map(|s| s.ip())
        .collect::<Vec<_>>();
    if enderecos.is_empty() {
        return Err(Falha::DestinoProibido);
    }
    // TODOS os endereços têm de passar: um nome que resolve para um público e um privado é
    // exatamente o truque que uma allowlist ingênua deixa passar.
    if !permitir_privado && enderecos.iter().any(|ip| !publico(*ip)) {
        return Err(Falha::DestinoProibido);
    }
    Ok(())
}

/// `true` só para endereço que é seguro buscar: nada de laço, rede privada, link-local,
/// multicast, documentação ou endereço não especificado.
fn publico(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            !(v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_broadcast()
                || v.is_documentation()
                || v.is_multicast()
                || v.is_unspecified()
                // 100.64/10 (CGNAT) e 192.0.0/24, que `is_private` não cobre.
                || (v.octets()[0] == 100 && (64..128).contains(&v.octets()[1]))
                || v.octets()[0] == 0)
        }
        IpAddr::V6(v) => {
            !(v.is_loopback()
                || v.is_multicast()
                || v.is_unspecified()
                // fe80::/10 link-local e fc00::/7 ULA.
                || (v.segments()[0] & 0xffc0) == 0xfe80
                || (v.segments()[0] & 0xfe00) == 0xfc00
                // IPv4 mapeado: decide pelo IPv4 de dentro, senão ::ffff:127.0.0.1 passaria.
                || v.to_ipv4_mapped().is_some_and(|v4| !publico(IpAddr::V4(v4))))
        }
    }
}

pub async fn registrar(
    pool: &SqlitePool,
    jogador_id: &str,
    url: &str,
    permitir_http: bool,
    permitir_privado: bool,
) -> R<Webhook> {
    validar_destino(url, permitir_http, permitir_privado).await?;

    let (quantos,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM webhooks WHERE jogador_id = ?1")
        .bind(jogador_id)
        .fetch_one(pool)
        .await?;
    if quantos >= LIMITE_POR_JOGADOR {
        return Err(Falha::LimiteDeWebhooks);
    }

    let id = uuid::Uuid::now_v7().to_string();
    let segredo = base16ct::lower::encode_string(&rand::random::<[u8; 32]>());
    let criado_em = bd::agora();
    sqlx::query(
        "INSERT INTO webhooks (id, jogador_id, url, segredo, criado_em)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&id)
    .bind(jogador_id)
    .bind(url)
    .bind(&segredo)
    .bind(&criado_em)
    .execute(pool)
    .await?;

    Ok(Webhook {
        id,
        url: url.to_string(),
        criado_em,
        segredo: Some(segredo),
    })
}

pub async fn listar(pool: &SqlitePool, jogador_id: &str) -> R<Vec<Webhook>> {
    let linhas: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, url, criado_em FROM webhooks WHERE jogador_id = ?1 ORDER BY criado_em",
    )
    .bind(jogador_id)
    .fetch_all(pool)
    .await?;
    Ok(linhas
        .into_iter()
        .map(|(id, url, criado_em)| Webhook {
            id,
            url,
            criado_em,
            segredo: None,
        })
        .collect())
}

pub async fn remover(pool: &SqlitePool, jogador_id: &str, id: &str) -> R<()> {
    let r = sqlx::query("DELETE FROM webhooks WHERE id = ?1 AND jogador_id = ?2")
        .bind(id)
        .bind(jogador_id)
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        // Inclui o caso de o webhook existir mas ser de outro jogador: do lado de fora, "não
        // achei" e "não é seu" têm de ser a mesma resposta, senão a rota conta quem existe.
        return Err(Falha::NaoAchei);
    }
    Ok(())
}

/// Dispara um evento para todos os webhooks destes jogadores. Não espera: a entrega vai para
/// uma tarefa própria, porque webhook lento de integrador não pode travar uma mesa (ADR-004).
pub fn disparar(
    pool: SqlitePool,
    cliente: reqwest::Client,
    jogadores: Vec<String>,
    evento: &'static str,
    corpo: serde_json::Value,
) {
    tokio::spawn(async move {
        let corpo = match serde_json::to_vec(&corpo) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(erro = %e, "corpo de webhook não serializou");
                return;
            }
        };
        for jogador_id in jogadores {
            let alvos: Vec<(String, String, String)> =
                match sqlx::query_as("SELECT id, url, segredo FROM webhooks WHERE jogador_id = ?1")
                    .bind(&jogador_id)
                    .fetch_all(&pool)
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::error!(erro = %e, "não consegui ler webhooks");
                        continue;
                    }
                };
            for (id, url, segredo) in alvos {
                entregar(&cliente, &id, &url, &segredo, evento, &corpo).await;
            }
        }
    });
}

pub fn assinar(segredo: &str, corpo: &[u8]) -> String {
    let mut mac = <Hmac<Sha256>>::new_from_slice(segredo.as_bytes())
        .expect("HMAC aceita chave de qualquer tamanho");
    mac.update(corpo);
    format!(
        "sha256={}",
        base16ct::lower::encode_string(&mac.finalize().into_bytes())
    )
}

async fn entregar(
    cliente: &reqwest::Client,
    id: &str,
    url: &str,
    segredo: &str,
    evento: &'static str,
    corpo: &[u8],
) {
    let assinatura = assinar(segredo, corpo);
    // Uma tentativa e uma repetição. Entrega garantida pediria tabela de entregas e um
    // trabalhador — camada inteira para dois eventos de um jogo de cartas (ADR-004).
    for tentativa in 1..=2u8 {
        let entrega = uuid::Uuid::new_v4().to_string();
        let r = cliente
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-Truco-Evento", evento)
            .header("X-Truco-Entrega", &entrega)
            .header("X-Truco-Assinatura", &assinatura)
            .timeout(TEMPO_LIMITE)
            .body(corpo.to_vec())
            .send()
            .await;
        match r {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!(webhook = id, evento, status = %resp.status(), "entregue");
                return;
            }
            Ok(resp) => tracing::warn!(
                webhook = id, evento, tentativa, status = %resp.status(), "recusado"
            ),
            Err(e) => tracing::warn!(webhook = id, evento, tentativa, erro = %e, "falhou"),
        }
        if tentativa == 1 {
            tokio::time::sleep(ESPERA_ANTES_DE_REPETIR).await;
        }
    }
    tracing::warn!(webhook = id, evento, "desisti depois de duas tentativas");
}
