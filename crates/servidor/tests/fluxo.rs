//! Prova de que o jogo funciona de ponta a ponta: servidor no processo, clientes de verdade
//! por HTTP e WebSocket, partida completa até alguém chegar a 12.
//!
//! Nada aqui é mock. O servidor é o mesmo `roteador` do binário, o WebSocket é
//! `tokio-tungstenite` falando com o `axum::extract::ws`, e o banco é um SQLite em arquivo
//! temporário — não `sqlite::memory:`, porque com um *pool* cada conexão em memória é um
//! banco diferente e o teste passaria por acidente.

use serde_json::{Value, json};
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

#[tokio::test(flavor = "multi_thread")]
async fn quem_para_de_jogar_perde_no_prazo_e_o_dinheiro_do_outro_nao_fica_preso() {
    // "O jogador cai" e "o jogador **para**" são coisas diferentes: queda fecha o socket e
    // a mesa trata como abandono, mas quem deixa a aba aberta e não age nunca fecha nada.
    // Sem o relógio, a mesa ficava de pé para sempre com a aposta do adversário dentro.
    let (endereco, _dir) =
        comum::servidor_com_prazo(false, std::time::Duration::from_millis(400)).await;
    let ana = Cliente::registrar(endereco, "ana").await;
    let bia = Cliente::registrar(endereco, "bia").await;

    let mut s1 = ana.conectar("1x1", 100).await;
    let mut s2 = bia.conectar("1x1", 100).await;

    // Os dois recebem a mesa e o primeiro estado — e aí nenhum dos dois age.
    for s in [&mut s1, &mut s2] {
        loop {
            if proxima(s).await["t"] == "estado" {
                break;
            }
        }
    }

    // A mão 0 é puxada pelo assento 0, logo é a dupla 0 que está devendo a ação e perde.
    let fim = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let m = proxima(&mut s2).await;
            if m["t"] == "fim" {
                return m;
            }
        }
    })
    .await
    .expect("a mesa tem de fechar sozinha no prazo");

    assert_eq!(
        fim["vencedora"], 1,
        "quem estava devendo a jogada (dupla 0) perde"
    );

    // E o dinheiro saiu de onde estava preso: a soma volta a 2000, com 1100 e 900.
    let mut saldos = [ana.moedas().await, bia.moedas().await];
    saldos.sort_unstable();
    assert_eq!(
        saldos,
        [900, 1100],
        "o saldo liquidou em vez de ficar retido"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn oito_abas_apostando_tudo_ao_mesmo_tempo_nao_deixam_o_saldo_negativo() {
    // Há duas defesas contra apostar o que não se tem: a conferência em `ws::entrar`, que lê
    // o saldo da sessão, e a condição `AND moedas >= ?2` dentro do próprio UPDATE. A
    // primeira é a que dá a mensagem bonita; a **segunda** é a que vale, porque oito abas
    // simultâneas leem todas o mesmo saldo de 1000 e passam todas pela primeira.
    //
    // Este teste existe porque eu apaguei a condição do UPDATE e **nenhum teste ficou
    // vermelho** — a conferência de `ws::entrar` escondia a falta dela. Sem este teste, a
    // defesa que realmente importa não estava testada.
    let (endereco, _dir) = comum::servidor(false).await;
    let ana = Cliente::registrar(endereco, "ana").await;
    assert_eq!(ana.moedas().await, 1000);

    // Os sockets ficam **vivos** durante a conferência. Na primeira versão deste teste eu
    // os deixava cair no fim de cada tentativa, e aí a devolução de quem desiste da fila já
    // tinha repago a aposta antes de eu ler o saldo — comportamento certo, teste errado.
    let tentativas = (0..8).map(|_| {
        let token = ana.token.clone();
        async move {
            let url = format!("ws://{endereco}/ws?modo=2x2&aposta=1000");
            let mut req = url.into_client_request().unwrap();
            req.headers_mut().insert(
                "Cookie",
                HeaderValue::from_str(&format!("sessao={token}")).unwrap(),
            );
            tokio_tungstenite::connect_async(req)
                .await
                .ok()
                .map(|(s, _)| s)
        }
    });
    let abertas: Vec<_> = futures_util::future::join_all(tentativas)
        .await
        .into_iter()
        .flatten()
        .collect();

    let saldo = ana.moedas().await;
    assert!(
        saldo >= 0,
        "o saldo não pode ficar negativo, e ficou {saldo}"
    );
    assert_eq!(
        abertas.len(),
        1,
        "só uma das oito podia ser cobrada; passaram {}",
        abertas.len()
    );
    assert_eq!(saldo, 0, "exatamente uma aposta de 1000 foi cobrada");
    drop(abertas);
}

#[tokio::test(flavor = "multi_thread")]
async fn pedir_truco_e_correr_pelo_protocolo() {
    // O jogador automático dos outros testes aceita truco mas nunca **pede** — então o
    // caminho pedir → responder nunca era exercitado pelo WebSocket, só pelos testes de
    // regra. Esta é a lacuna que este teste fecha.
    let (endereco, _dir) = comum::servidor(false).await;
    let ana = Cliente::registrar(endereco, "ana").await;
    let bia = Cliente::registrar(endereco, "bia").await;
    let mut s1 = ana.conectar("1x1", 0).await;
    let mut s2 = bia.conectar("1x1", 0).await;

    /// Lê até o primeiro `estado` e devolve-o.
    async fn estado(s: &mut comum::Socket) -> Value {
        loop {
            let m = proxima(s).await;
            if m["t"] == "estado" {
                return m;
            }
        }
    }

    let e1 = estado(&mut s1).await;
    let e2 = estado(&mut s2).await;
    // Quem puxa a mão 0 é o assento 0; a outra ponta não pode agir.
    let (de_quem_eh_a_vez, de_quem_nao_eh) = if e1["vez"] == e1["assento"] {
        (&mut s1, &mut s2)
    } else {
        (&mut s2, &mut s1)
    };
    let (dono, outro) = if e1["vez"] == e1["assento"] {
        (e1, e2)
    } else {
        (e2, e1)
    };
    assert_eq!(dono["valor"], 1, "a mão começa valendo 1 (R-15)");
    assert!(
        dono["acoes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "pedir"),
        "quem está na vez pode pedir truco"
    );
    assert!(
        outro["acoes"].as_array().unwrap().is_empty(),
        "quem não está na vez não tem ação: {outro:?}"
    );

    // Quem não está na vez tentando pedir recebe erro — e só ele recebe.
    comum::manda(de_quem_nao_eh, json!({"t":"pedir"})).await;
    let erro = loop {
        let m = proxima(de_quem_nao_eh).await;
        if m["t"] == "erro" {
            break m;
        }
    };
    assert_eq!(
        erro["erro"], "nao_eh_sua_vez",
        "R-16: só se pede na própria vez"
    );

    // Agora o pedido legítimo.
    comum::manda(de_quem_eh_a_vez, json!({"t":"pedir"})).await;
    let depois = estado(de_quem_nao_eh).await;
    let p = &depois["pendencia"];
    assert_eq!(p["valor_proposto"], 3, "truco propõe 3 (R-15)");
    assert_eq!(
        p["assento_respondente"], depois["assento"],
        "ADR-005: um respondente só"
    );
    let acoes: Vec<&str> = depois["acoes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        acoes,
        ["aceitar", "correr", "aumentar"],
        "as três respostas (R-17)"
    );

    // Corre: quem pediu leva 1, que é o valor de antes do pedido (R-18).
    comum::manda(de_quem_nao_eh, json!({"t":"correr"})).await;
    // Drena até o estado da mão **seguinte**: quem pediu ainda tinha na fila o estado
    // difundido no momento do pedido, e ler o primeiro `estado` pegaria aquele. Foi o que
    // esta asserção pegou na primeira execução.
    let novo = loop {
        let m = estado(de_quem_eh_a_vez).await;
        if m["numero_da_mao"] == 1 {
            break m;
        }
    };
    let placar: Vec<i64> = novo["placar"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(
        placar.iter().sum::<i64>(),
        1,
        "R-18: correr de um truco vale 1, placar {placar:?}"
    );
    assert_eq!(novo["valor"], 1, "R-21: a mão seguinte volta a valer 1");
    assert_eq!(novo["numero_da_mao"], 1, "a mão seguinte começou");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rodada_resolvida_continua_no_estado_com_as_cartas_e_quem_levou() {
    // Regressão do defeito relatado por quem jogou: "depois que jogo a carta não dá para
    // ver o que o outro jogou na sequência". A causa era de protocolo — o servidor limpava
    // a mesa ao resolver a rodada e o `estado` dizia quem levou **sem dizer com quais
    // cartas**, então nem um cliente perfeito conseguiria manter a rodada em tela.
    let (endereco, _dir) = comum::servidor(false).await;
    let ana = Cliente::registrar(endereco, "ana").await;
    let bia = Cliente::registrar(endereco, "bia").await;
    let mut s1 = ana.conectar("1x1", 0).await;
    let mut s2 = bia.conectar("1x1", 0).await;

    async fn estado(s: &mut comum::Socket) -> Value {
        loop {
            let m = proxima(s).await;
            if m["t"] == "estado" {
                return m;
            }
        }
    }

    let e1 = estado(&mut s1).await;
    let e2 = estado(&mut s2).await;
    assert_eq!(
        e1["rodadas"].as_array().unwrap().len(),
        0,
        "nenhuma rodada resolvida ainda"
    );

    // Joga a rodada 1 inteira, guardando o que cada um pôs na mesa.
    let (primeiro, segundo, carta_do_primeiro) = if e1["vez"] == e1["assento"] {
        (
            &mut s1,
            &mut s2,
            e1["minhas_cartas"][0].as_str().unwrap().to_string(),
        )
    } else {
        (
            &mut s2,
            &mut s1,
            e2["minhas_cartas"][0].as_str().unwrap().to_string(),
        )
    };
    comum::manda(primeiro, json!({"t":"jogar","indice":0,"coberta":false})).await;
    let depois = estado(segundo).await;
    let carta_do_segundo = depois["minhas_cartas"][0].as_str().unwrap().to_string();
    comum::manda(segundo, json!({"t":"jogar","indice":0,"coberta":false})).await;

    // O primeiro estado com a rodada já resolvida: ela tem de carregar as duas cartas.
    let resolvido = loop {
        let m = estado(primeiro).await;
        if !m["rodadas"].as_array().unwrap().is_empty() {
            break m;
        }
    };
    let r = &resolvido["rodadas"][0];
    let jogadas = r["jogadas"]
        .as_array()
        .expect("a rodada resolvida carrega as jogadas");
    assert_eq!(
        jogadas.len(),
        2,
        "as duas cartas da rodada, não só o vencedor"
    );
    let cartas: Vec<&str> = jogadas
        .iter()
        .map(|j| j["carta"].as_str().unwrap())
        .collect();
    assert!(
        cartas.contains(&carta_do_primeiro.as_str()),
        "a carta de quem puxou está lá"
    );
    assert!(
        cartas.contains(&carta_do_segundo.as_str()),
        "e a do adversário também"
    );

    // E diz **quem** levou, por assento — o que o cliente não pode derivar sem conhecer a
    // ordem de força (e ele não a conhece, por ADR-003).
    if r["vencedora"].is_null() {
        assert!(
            r["assento_vencedor"].is_null(),
            "empate não tem assento vencedor"
        );
    } else {
        let venceu = r["assento_vencedor"]
            .as_u64()
            .expect("quem levou, por assento");
        assert!(
            jogadas
                .iter()
                .any(|j| j["assento"].as_u64() == Some(venceu)),
            "o assento vencedor jogou nesta rodada"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn o_fim_da_mao_avisa_o_detalhe_inteiro_antes_de_a_mao_nova_comecar() {
    // A outra metade do mesmo defeito: quando a **mão** acaba, o `estado` seguinte já é da
    // mão nova, então a última rodada se perderia sem um aviso que a carregue.
    let (endereco, _dir) = comum::servidor(false).await;
    let ana = Cliente::registrar(endereco, "ana").await;
    let bia = Cliente::registrar(endereco, "bia").await;
    let s1 = ana.conectar("1x1", 0).await;
    let s2 = bia.conectar("1x1", 0).await;

    // Joga a partida inteira recolhendo os avisos `mao_resolvida`.
    let coletar = |mut s: comum::Socket| async move {
        let mut maos: Vec<Value> = Vec::new();
        loop {
            let m = proxima(&mut s).await;
            match m["t"].as_str().unwrap_or_default() {
                "fim" => return maos,
                "avisos" => {
                    for a in m["avisos"].as_array().unwrap() {
                        if a["aviso"] == "mao_resolvida" {
                            maos.push(a.clone());
                        }
                    }
                }
                "estado" => {
                    let acoes = m["acoes"].as_array().cloned().unwrap_or_default();
                    let tem = |x: &str| acoes.iter().any(|a| a == x);
                    if tem("onze_aceitar") {
                        comum::manda(&mut s, json!({"t":"onze","aceita":true})).await;
                    } else if tem("aceitar") {
                        comum::manda(&mut s, json!({"t":"aceitar"})).await;
                    } else if tem("jogar") {
                        comum::manda(&mut s, json!({"t":"jogar","indice":0,"coberta":false})).await;
                    }
                }
                _ => {}
            }
        }
    };
    let (maos, _) = tokio::join!(coletar(s1), coletar(s2));

    assert!(
        !maos.is_empty(),
        "a partida teve mãos, e cada uma avisou o seu detalhe"
    );
    for m in &maos {
        // Cada mão resolvida se descreve sozinha: é o que o histórico consome.
        assert!(m["vira"].as_str().is_some(), "a vira da mão");
        assert!(
            m["manilha"].as_str().is_some(),
            "a manilha, como número e não carta"
        );
        let valor = m["valor"].as_u64().expect("quanto a mão valia");
        assert!(
            [1, 3, 6, 9, 12].contains(&valor),
            "R-15: valor é 1/3/6/9/12, veio {valor}"
        );
        let rodadas = m["rodadas"].as_array().expect("as rodadas da mão");
        assert!(!rodadas.is_empty() || m["fim"]["como"] != "cartas");
        for r in rodadas {
            for j in r["jogadas"].as_array().unwrap() {
                // Carta de costas continua escondida **depois** de a mão acabar (R-14).
                if j["coberta"] == true {
                    assert!(
                        j["carta"].is_null(),
                        "a rodada fechar não revela carta coberta"
                    );
                }
            }
        }
        assert_eq!(
            m["placar"].as_array().unwrap().len(),
            2,
            "o placar depois da mão"
        );
    }
    // O número da mão cresce de um em um, começando em zero.
    let numeros: Vec<u64> = maos.iter().map(|m| m["numero"].as_u64().unwrap()).collect();
    assert_eq!(
        numeros,
        (0..numeros.len() as u64).collect::<Vec<_>>(),
        "{numeros:?}"
    );
}
