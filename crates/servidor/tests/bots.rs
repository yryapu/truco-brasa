//! Mesa de treino: um humano e bots nos assentos que faltam.
//!
//! O que importa provar aqui não é que o bot joga bem — é que (a) a partida **termina**
//! sozinha, chegando aos 12 pontos em vez de expirar no relógio, (b) o bot nunca tenta
//! jogada ilegal, e (c) **treino não move moeda nem ranking**, porque se movesse eu teria
//! criado exatamente o farm de reputação que a v1 declarou como risco conhecido.

use serde_json::Value;

mod comum;
use comum::{Cliente, jogar_ate_o_fim, proxima, servidor};

/// Conecta em treino, guarda a mensagem `mesa` e joga até o fim.
async fn treinar(c: &Cliente, modo: &str) -> (Value, Value) {
    let mut s = c.conectar_treino(modo).await;
    let mesa = loop {
        let m = proxima(&mut s).await;
        if m["t"] == "mesa" {
            break m;
        }
    };
    let fim = jogar_ate_o_fim(s, "humano").await;
    (mesa, fim)
}

#[tokio::test(flavor = "multi_thread")]
async fn treino_1x1_contra_bot_vai_ate_os_doze_sozinho() {
    let (endereco, _dir) = servidor(false).await;
    let ana = Cliente::registrar(endereco, "ana").await;

    let (mesa, fim) =
        tokio::time::timeout(std::time::Duration::from_secs(30), treinar(&ana, "1x1"))
            .await
            .expect("a partida de treino tem de terminar sozinha");

    // A mesa se identifica como treino, e diz quem é bot.
    assert_eq!(mesa["treino"], true, "mesa com bot é treino");
    let jogadores = mesa["jogadores"].as_array().unwrap();
    assert_eq!(jogadores.len(), 2);
    let bots: Vec<&Value> = jogadores.iter().filter(|j| j["bot"] == true).collect();
    assert_eq!(bots.len(), 1, "um bot no 1x1");
    assert!(
        bots[0]["apelido"].as_str().is_some_and(|a| !a.is_empty()),
        "o bot tem nome"
    );
    assert_eq!(
        jogadores.iter().filter(|j| j["bot"] == false).count(),
        1,
        "e um humano"
    );

    // Terminou **nas cartas**, não no relógio: alguém chegou a 12. Esta é a asserção que
    // impede um bot travado de passar como sucesso — travado, a mesa expiraria com placar
    // baixo e esta linha cairia.
    let placar: Vec<i64> = fim["placar"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    let vencedora = fim["vencedora"].as_u64().unwrap() as usize;
    assert!(
        placar[vencedora] >= 12,
        "a partida tem de chegar aos 12 e não expirar; placar {placar:?}"
    );

    // Treino não paga.
    assert_eq!(fim["treino"], true);
    assert_eq!(fim["ganho"], 0, "treino não move moeda");
    let ficha = ana.eu().await;
    assert_eq!(ficha["moedas"], 1000, "o saldo não mudou");
    assert_eq!(ficha["partidas"], 0, "treino não conta partida");
    assert_eq!(ficha["vitorias"], 0, "nem vitória");
    assert_eq!(ficha["derrotas"], 0, "nem derrota");

    // E o emblema de estreante continua lá, justamente porque nada contou.
    let chaves: Vec<&str> = ficha["emblemas"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["chave"].as_str().unwrap())
        .collect();
    assert!(
        chaves.contains(&"estreante"),
        "treino não tira o estreante: {chaves:?}"
    );
    assert!(!chaves.contains(&"batismo"), "e não dá batismo");
}

#[tokio::test(flavor = "multi_thread")]
async fn treino_2x2_enche_a_mesa_com_tres_bots() {
    let (endereco, _dir) = servidor(false).await;
    let bia = Cliente::registrar(endereco, "bia").await;

    let (mesa, fim) =
        tokio::time::timeout(std::time::Duration::from_secs(45), treinar(&bia, "2x2"))
            .await
            .expect("a partida de treino 2x2 tem de terminar sozinha");

    let jogadores = mesa["jogadores"].as_array().unwrap();
    assert_eq!(jogadores.len(), 4);
    assert_eq!(
        jogadores.iter().filter(|j| j["bot"] == true).count(),
        3,
        "três bots"
    );
    // O parceiro do humano (assento 0) é o assento 2, e é bot.
    let parceiro = jogadores.iter().find(|j| j["assento"] == 2).unwrap();
    assert_eq!(parceiro["bot"], true);
    assert_eq!(
        parceiro["equipe"], jogadores[0]["equipe"],
        "parceiro na mesma dupla"
    );

    let placar: Vec<i64> = fim["placar"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    let vencedora = fim["vencedora"].as_u64().unwrap() as usize;
    assert!(placar[vencedora] >= 12, "chegou aos 12; placar {placar:?}");
    assert_eq!(bia.eu().await["moedas"], 1000);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_aposta_no_treino_eh_forcada_a_zero_mesmo_se_o_cliente_pedir_outra() {
    // Um cliente modificado pode mandar `aposta=500&bots=1`. Se o servidor respeitasse,
    // ganhar de um bot imprimiria moeda — o bot não tem saldo de onde ela saísse.
    let (endereco, _dir) = servidor(false).await;
    let caio = Cliente::registrar(endereco, "caio").await;

    let mut s = caio.conectar_cru("1x1", 500, true).await;
    let mesa = loop {
        let m = proxima(&mut s).await;
        if m["t"] == "mesa" {
            break m;
        }
    };
    assert_eq!(mesa["aposta"], 0, "a aposta de treino é forçada a zero");
    assert_eq!(mesa["treino"], true);
    assert_eq!(caio.moedas().await, 1000, "e nada foi debitado");
}
