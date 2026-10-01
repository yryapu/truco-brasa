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

    // `truco --saude` serve ao HEALTHCHECK do container: o binário pergunta a si mesmo e
    // sai 0 ou 1. Sem isso a imagem final precisaria de `curl` instalado só para isso, e
    // ferramenta de rede numa imagem de produção é superfície que não compra nada.
    if std::env::args().any(|a| a == "--saude") {
        let alvo = format!("http://{}/saude", endereco.replace("0.0.0.0", "127.0.0.1"));
        let ok = reqwest::Client::new()
            .get(&alvo)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success());
        std::process::exit(i32::from(!ok));
    }
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
    // 60 s por ação é generoso para quem está pensando e curto para quem foi embora.
    let prazo = std::env::var("TRUCO_PRAZO_SEGUNDOS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let config = Config {
        cookie_seguro: seguro,
        permitir_http_webhook: !seguro,
        permitir_destino_privado: webhook_local,
        prazo_de_jogada: std::time::Duration::from_secs(prazo),
        // O bot "pensa" por padrão, porque sem pausa a mão resolve entre dois quadros e
        // o jogador não vê o que aconteceu. O teste de interface baixa isto para não
        // esperar por atraso cosmético — mesma razão dos testes em Rust, que usam zero.
        pausa_do_bot: std::time::Duration::from_millis(
            std::env::var("TRUCO_PAUSA_BOT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1_100),
        ),
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
