//! Sobe o servidor. Tudo por variável de ambiente, com padrão que funciona sem configurar.

use truco_servidor::estado::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,truco_servidor=debug".into()),
        )
        .init();

    let endereco = std::env::var("TRUCO_ENDERECO").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let banco = std::env::var("TRUCO_BANCO").unwrap_or_else(|_| "sqlite://truco.db".into());
    // Padrão: desenvolvimento. Em produção atrás de TLS, `TRUCO_SEGURO=1` liga o `Secure`
    // no cookie e fecha o webhook em `http://`.
    let seguro = std::env::var("TRUCO_SEGURO").is_ok_and(|v| v == "1" || v == "true");

    // A chave de destino local existe só para testar entrega de webhook na própria máquina.
    // Junto de TRUCO_SEGURO=1 ela seria um SSRF aberto em produção, então isto recusa a
    // combinação em vez de preferir uma das duas.
    let webhook_local = std::env::var("TRUCO_WEBHOOK_LOCAL").is_ok_and(|v| v == "1");
    if seguro && webhook_local {
        anyhow::bail!(
            "TRUCO_WEBHOOK_LOCAL=1 com TRUCO_SEGURO=1 abriria SSRF em produção; escolha um"
        );
    }
    let config = Config {
        cookie_seguro: seguro,
        permitir_http_webhook: !seguro,
        permitir_destino_privado: webhook_local,
    };
    let app = truco_servidor::montar(&banco, config).await?;

    let escuta = tokio::net::TcpListener::bind(&endereco).await?;
    tracing::info!(%endereco, %banco, seguro, "truco de pé");
    axum::serve(escuta, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("encerrando");
        })
        .await?;
    Ok(())
}
