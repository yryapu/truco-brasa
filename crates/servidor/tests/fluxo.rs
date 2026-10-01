//! Prova de que o jogo funciona de ponta a ponta: servidor no processo, clientes de verdade
//! por HTTP e WebSocket, partida completa até alguém chegar a 12.
//!
//! Nada aqui é mock. O servidor é o mesmo `roteador` do binário, o WebSocket é
//! `tokio-tungstenite` falando com o `axum::extract::ws`, e o banco é um SQLite em arquivo
//! temporário — não `sqlite::memory:`, porque com um *pool* cada conexão em memória é um
//! banco diferente e o teste passaria por acidente.

use serde_json::Value;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

mod comum;
use comum::{Cliente, jogar_ate_o_fim, proxima, servidor};

#[tokio::test(flavor = "multi_thread")]
async fn uma_partida_1x1_inteira_do_cadastro_ao_pagamento() {
    let (endereco, _dir) = servidor(false).await;
    let ana = Cliente::registrar(endereco, "ana").await;
    let bia = Cliente::registrar(endereco, "bia").await;

    let aposta = 100;
    let s1 = ana.conectar("1x1", aposta).await;
    let s2 = bia.conectar("1x1", aposta).await;

    // A aposta sai do saldo na entrada, antes de haver mesa.
    assert_eq!(
        ana.moedas().await,
        900,
        "a aposta é debitada ao entrar na fila"
    );

    let (f1, f2) = tokio::join!(jogar_ate_o_fim(s1, "ana"), jogar_ate_o_fim(s2, "bia"));

    // As duas pontas concordam sobre quem ganhou e sobre o placar.
    assert_eq!(
        f1["vencedora"], f2["vencedora"],
        "as duas pontas viram o mesmo vencedor"
    );
    assert_eq!(f1["placar"], f2["placar"]);
    let placar = f1["placar"].as_array().unwrap();
    let p: Vec<i64> = placar.iter().map(|v| v.as_i64().unwrap()).collect();
    assert!(
        p[0] >= 12 || p[1] >= 12,
        "R-27: alguém chegou a 12, placar {p:?}"
    );

    // O dinheiro fecha: o bolo de 200 foi inteiro para quem ganhou.
    let (ma, mb) = (ana.moedas().await, bia.moedas().await);
    assert_eq!(ma + mb, 2000, "o jogo não cria nem destrói moeda");
    let mut saldos = [ma, mb];
    saldos.sort_unstable();
    assert_eq!(saldos, [900, 1100], "vencedor +100, perdedor -100");

    // A estatística fechou e o emblema de batismo apareceu para quem ganhou.
    for c in [&ana, &bia] {
        let f = c.eu().await;
        assert_eq!(f["partidas"], 1, "{} jogou uma partida", c.apelido);
        let chaves: Vec<&str> = f["emblemas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["chave"].as_str().unwrap())
            .collect();
        assert!(!chaves.contains(&"estreante"), "já não é estreante");
        if f["vitorias"].as_i64() == Some(1) {
            assert!(chaves.contains(&"batismo"), "quem ganhou leva o batismo");
        }
    }

    // O ranking é público e **não** carrega o id interno de ninguém.
    let r: Value = reqwest::get(format!("http://{endereco}/api/ranking"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for linha in r.as_array().unwrap() {
        assert!(
            linha.get("id").is_none(),
            "o ranking público não expõe o id"
        );
        assert!(linha.get("apelido").is_some());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn uma_partida_2x2_inteira_com_quatro_jogadores() {
    let (endereco, _dir) = servidor(false).await;
    let mut sockets = Vec::new();
    let mut clientes = Vec::new();
    for nome in ["ana", "bia", "caio", "duda"] {
        let c = Cliente::registrar(endereco, nome).await;
        sockets.push(c.conectar("2x2", 50).await);
        clientes.push(c);
    }

    let fins = futures_util::future::join_all(sockets.into_iter().enumerate().map(|(i, s)| {
        let rotulo = format!("assento {i}");
        async move { jogar_ate_o_fim(s, &rotulo).await }
    }))
    .await;

    let vencedora = fins[0]["vencedora"].clone();
    for f in &fins {
        assert_eq!(
            f["vencedora"], vencedora,
            "os quatro viram o mesmo vencedor"
        );
        assert_eq!(f["placar"], fins[0]["placar"]);
    }
    // Dois ganham, dois perdem, e a soma dos saldos não muda.
    let mut total = 0;
    for c in &clientes {
        total += c.moedas().await;
    }
    assert_eq!(total, 4000, "o jogo não cria nem destrói moeda em 2x2");
}

#[tokio::test(flavor = "multi_thread")]
async fn um_assento_nunca_recebe_a_carta_do_outro() {
    let (endereco, _dir) = servidor(false).await;
    let ana = Cliente::registrar(endereco, "ana").await;
    let bia = Cliente::registrar(endereco, "bia").await;
    let mut s1 = ana.conectar("1x1", 0).await;
    let mut s2 = bia.conectar("1x1", 0).await;

    // Primeiro `estado` de cada um, depois de passar pelas mensagens de fila e mesa.
    let mut cartas = Vec::new();
    for s in [&mut s1, &mut s2] {
        loop {
            let m = proxima(s).await;
            if m["t"] == "estado" {
                let minhas: Vec<String> = m["minhas_cartas"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c.as_str().unwrap().to_string())
                    .collect();
                assert_eq!(minhas.len(), 3, "R-09: três cartas");
                assert!(
                    m["cartas_do_parceiro"].is_null(),
                    "1x1 não tem parceiro, e fora da mão de onze ninguém vê ninguém"
                );
                let texto = m.to_string();
                cartas.push((minhas, texto));
                break;
            }
        }
    }
    let (minhas_da_ana, estado_da_ana) = &cartas[0];
    let (minhas_da_bia, estado_da_bia) = &cartas[1];

    // Nenhuma carta é a mesma nas duas mãos (o baralho não repete).
    for c in minhas_da_ana {
        assert!(
            !minhas_da_bia.contains(c),
            "a carta {c} apareceu nas duas mãos"
        );
    }
    // E o estado que cada um recebe não contém, em nenhum campo, carta do outro.
    for c in minhas_da_bia {
        assert!(
            !estado_da_ana.contains(c.as_str()),
            "ana recebeu {c}, que é da bia"
        );
    }
    for c in minhas_da_ana {
        assert!(
            !estado_da_bia.contains(c.as_str()),
            "bia recebeu {c}, que é da ana"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nao_se_aposta_mais_do_que_se_tem_e_a_desistencia_devolve() {
    let (endereco, _dir) = servidor(false).await;
    let ana = Cliente::registrar(endereco, "ana").await;

    // Aposta maior que o saldo: o upgrade do WebSocket é recusado.
    let url = format!("ws://{endereco}/ws?modo=1x1&aposta=5000");
    let mut req = url.into_client_request().unwrap();
    req.headers_mut().insert(
        "Cookie",
        HeaderValue::from_str(&format!("sessao={}", ana.token)).unwrap(),
    );
    assert!(
        tokio_tungstenite::connect_async(req).await.is_err(),
        "aposta acima do saldo não deve abrir a mesa"
    );
    assert_eq!(ana.moedas().await, 1000, "e não cobra nada");

    // Sem cookie, nem conecta.
    let url = format!("ws://{endereco}/ws?modo=1x1&aposta=10");
    assert!(
        tokio_tungstenite::connect_async(url).await.is_err(),
        "WebSocket sem sessão tem de ser recusado"
    );

    // Entra na fila, espera, e fecha a aba: a aposta volta.
    let s = ana.conectar("1x1", 300).await;
    // Dá tempo de o débito e a entrada na fila acontecerem antes de fechar.
    for _ in 0..20 {
        if ana.moedas().await == 700 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(ana.moedas().await, 700, "a aposta sai ao entrar na fila");
    drop(s);
    let mut devolveu = false;
    for _ in 0..40 {
        if ana.moedas().await == 1000 {
            devolveu = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(devolveu, "quem fecha a aba na fila tem a aposta devolvida");
}
