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

    let config = Config { cookie_seguro: seguro, permitir_http_webhook: !seguro };
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
