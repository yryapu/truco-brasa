//! Prova de que o webhook sai, chega assinado, e que a assinatura confere.
//!
//! O receptor é um servidor axum de verdade em `127.0.0.1`, o que obriga a ligar
//! `permitir_destino_privado` — a chave que em produção é proibida junto de `TRUCO_SEGURO`.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use serde_json::{Value, json};

mod comum;
use comum::{Cliente, jogar_ate_o_fim, servidor};

type Recebidas = Arc<Mutex<Vec<(String, String, String)>>>;

/// Receptor de webhook: guarda evento, assinatura e corpo cru de cada entrega.
async fn receber(
    State(caixa): State<Recebidas>,
    headers: HeaderMap,
    corpo: String,
) -> &'static str {
    let pega = |n: &str| {
        headers
            .get(n)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    caixa
        .lock()
        .unwrap()
        .push((pega("x-truco-evento"), pega("x-truco-assinatura"), corpo));
    "ok"
}

#[tokio::test(flavor = "multi_thread")]
async fn o_webhook_chega_assinado_nos_dois_eventos_da_partida() {
    let caixa: Recebidas = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/entrega", post(receber))
        .with_state(caixa.clone());
    let escuta = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let alvo = format!("http://{}/entrega", escuta.local_addr().unwrap());
    tokio::spawn(async move {
        let _ = axum::serve(escuta, app).await;
    });

    let (endereco, _dir) = servidor(true).await;
    let ana = Cliente::registrar(endereco, "ana").await;
    let bia = Cliente::registrar(endereco, "bia").await;

    // Registra o webhook e guarda o segredo — ele aparece uma vez só.
    let w: Value = ana
        .post("/api/webhooks", json!({"url": alvo}))
        .await
        .json()
        .await
        .expect("json");
    let segredo = w["segredo"]
        .as_str()
        .expect("o registro devolve o segredo")
        .to_string();

    // A listagem **não** devolve o segredo de volta.
    let lista: Value = ana.get("/api/webhooks").await.json().await.unwrap();
    assert_eq!(lista.as_array().unwrap().len(), 1);
    assert!(
        lista[0].get("segredo").is_none(),
        "o segredo não volta na listagem"
    );

    let s1 = ana.conectar("1x1", 10).await;
    let s2 = bia.conectar("1x1", 10).await;
    let _ = tokio::join!(jogar_ate_o_fim(s1, "ana"), jogar_ate_o_fim(s2, "bia"));

    // A entrega é assíncrona de propósito (ADR-004), então espera-se por ela.
    let mut entregas = Vec::new();
    for _ in 0..60 {
        entregas = caixa.lock().unwrap().clone();
        if entregas.len() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let eventos: Vec<&str> = entregas.iter().map(|(e, _, _)| e.as_str()).collect();
    assert!(
        eventos.contains(&"partida.comecou"),
        "faltou partida.comecou: {eventos:?}"
    );
    assert!(
        eventos.contains(&"partida.terminou"),
        "faltou partida.terminou: {eventos:?}"
    );

    for (evento, assinatura, corpo) in &entregas {
        // A assinatura é do corpo **exato**, e confere com o segredo que o registro deu.
        let esperada = truco_servidor::webhooks::assinar(&segredo, corpo.as_bytes());
        assert_eq!(
            assinatura, &esperada,
            "HMAC-SHA256 do corpo não confere no evento {evento}"
        );
        assert!(assinatura.starts_with("sha256="));
        // E o corpo diz o que o protocolo promete.
        let v: Value = serde_json::from_str(corpo).expect("corpo é JSON");
        assert_eq!(v["evento"], evento.as_str());
        assert!(v["partida"]["id"].is_string());
        if evento == "partida.terminou" {
            // O "resultado" do enunciado é o corpo deste evento, não um terceiro evento.
            let r = &v["resultado"];
            assert!(r["equipe_vencedora"].is_number());
            assert_eq!(r["placar"].as_array().unwrap().len(), 2);
            assert_eq!(
                r["vencedores"].as_array().unwrap().len(),
                1,
                "1x1: um vencedor"
            );
            assert_eq!(r["perdedores"].as_array().unwrap().len(), 1);
            assert_eq!(r["moedas_por_vencedor"], 20);
        }
    }

    // Uma assinatura com o segredo errado não confere — senão o teste acima não provaria nada.
    let (_, assinatura, corpo) = &entregas[0];
    assert_ne!(
        &truco_servidor::webhooks::assinar("segredo-errado", corpo.as_bytes()),
        assinatura,
        "a verificação tem de distinguir o segredo certo do errado"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guarda_de_destino_recusa_a_propria_rede() {
    let (endereco, _dir) = servidor(false).await;
    let ana = Cliente::registrar(endereco, "ana").await;

    for url in [
        "http://127.0.0.1:9/x",
        "http://localhost:9/x",
        "http://169.254.169.254/latest/meta-data/",
        "http://[::1]:9/x",
        "http://10.0.0.5/x",
        "http://192.168.1.1/x",
        "http://172.16.0.1/x",
        "ftp://exemplo.com/x",
        "file:///etc/passwd",
        "nao-e-url",
    ] {
        let r = ana.post("/api/webhooks", json!({"url": url})).await;
        assert_eq!(r.status(), 400, "{url} deveria ser recusado");
        let v: Value = r.json().await.unwrap();
        assert!(
            v["erro"] == "destino_proibido" || v["erro"] == "url_invalida",
            "{url} deu {}",
            v["erro"]
        );
    }
    // E nenhum deles ficou registrado.
    let lista: Value = ana.get("/api/webhooks").await.json().await.unwrap();
    assert!(lista.as_array().unwrap().is_empty(), "nada foi registrado");
}

#[tokio::test(flavor = "multi_thread")]
async fn o_webhook_de_um_jogador_nao_eh_visivel_nem_removivel_por_outro() {
    let (endereco, _dir) = servidor(true).await;
    let ana = Cliente::registrar(endereco, "ana").await;
    let bia = Cliente::registrar(endereco, "bia").await;

    let w: Value = ana
        .post("/api/webhooks", json!({"url": "http://127.0.0.1:9/x"}))
        .await
        .json()
        .await
        .unwrap();
    let id = w["id"].as_str().unwrap();

    let lista: Value = bia.get("/api/webhooks").await.json().await.unwrap();
    assert!(
        lista.as_array().unwrap().is_empty(),
        "bia não vê o webhook da ana"
    );

    // E a tentativa de remover devolve "não achei" — a mesma resposta de um id inexistente,
    // para que a rota não conte a quem pergunta o que existe.
    let r = bia.delete(&format!("/api/webhooks/{id}")).await;
    assert_eq!(r.status(), 404);
    let inexistente = bia.delete("/api/webhooks/nao-existe").await;
    assert_eq!(
        inexistente.status(),
        404,
        "mesma resposta para id que não existe"
    );

    // O da ana continua lá.
    let lista: Value = ana.get("/api/webhooks").await.json().await.unwrap();
    assert_eq!(lista.as_array().unwrap().len(), 1);
}
