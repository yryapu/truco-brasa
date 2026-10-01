//! O banco: SQLite por `sqlx`. Um arquivo, um processo, ACID (ADR-001).
//!
//! Usamos `sqlx::query` em tempo de execução, não as macros `query!` conferidas em tempo de
//! compilação. A razão é operacional: as macros exigem `DATABASE_URL` com um banco real
//! **durante o build**, o que faria o `docker build` depender de um banco. O custo aceito é
//! que um erro de SQL aparece no teste e não no compilador — e é por isso que toda consulta
//! daqui é exercitada por teste.

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

pub const MOEDAS_INICIAIS: i64 = 1000;

pub async fn abrir(url: &str) -> Result<SqlitePool> {
    let opcoes: SqliteConnectOptions = url.parse()?;
    let opcoes = opcoes
        .create_if_missing(true)
        // WAL: leitura não bloqueia escrita. Importa porque o ranking é lido com frequência
        // enquanto partidas terminam.
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(8).connect_with(opcoes).await?;
    criar_tabelas(&pool).await?;
    Ok(pool)
}

/// Esquema idempotente. Sem diretório de migrações: a v1 tem um esquema, e `IF NOT EXISTS`
/// é a migração dela. Na primeira mudança incompatível entra `sqlx::migrate!`.
async fn criar_tabelas(pool: &SqlitePool) -> Result<()> {
    let ddl = [
        "CREATE TABLE IF NOT EXISTS jogadores (
            id               TEXT PRIMARY KEY,
            apelido          TEXT NOT NULL,
            senha_hash       TEXT NOT NULL,
            moedas           INTEGER NOT NULL DEFAULT 1000,
            partidas         INTEGER NOT NULL DEFAULT 0,
            vitorias         INTEGER NOT NULL DEFAULT 0,
            derrotas         INTEGER NOT NULL DEFAULT 0,
            sequencia        INTEGER NOT NULL DEFAULT 0,
            melhor_sequencia INTEGER NOT NULL DEFAULT 0,
            lavadas          INTEGER NOT NULL DEFAULT 0,
            criado_em        TEXT NOT NULL
        )",
        // Apelido único sem distinguir maiúscula: 'Ana' e 'ana' são a mesma pessoa para
        // quem vai digitar, então deixar as duas existirem seria convite a falsificação.
        "CREATE UNIQUE INDEX IF NOT EXISTS jogadores_apelido ON jogadores (apelido COLLATE NOCASE)",
        "CREATE TABLE IF NOT EXISTS sessoes (
            token_hash TEXT PRIMARY KEY,
            jogador_id TEXT NOT NULL REFERENCES jogadores(id) ON DELETE CASCADE,
            criado_em  TEXT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS webhooks (
            id         TEXT PRIMARY KEY,
            jogador_id TEXT NOT NULL REFERENCES jogadores(id) ON DELETE CASCADE,
            url        TEXT NOT NULL,
            segredo    TEXT NOT NULL,
            criado_em  TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS webhooks_jogador ON webhooks (jogador_id)",
        "CREATE TABLE IF NOT EXISTS partidas (
            id               TEXT PRIMARY KEY,
            modo             TEXT NOT NULL,
            aposta           INTEGER NOT NULL,
            equipe_vencedora INTEGER,
            placar0          INTEGER,
            placar1          INTEGER,
            comecou_em       TEXT NOT NULL,
            terminou_em      TEXT
        )",
    ];
    for s in ddl {
        sqlx::query(s).execute(pool).await?;
    }
    Ok(())
}

/// A ficha de um jogador. É o que `GET /api/eu` e o ranking devolvem.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Jogador {
    pub id: String,
    pub apelido: String,
    pub moedas: i64,
    pub partidas: i64,
    pub vitorias: i64,
    pub derrotas: i64,
    pub melhor_sequencia: i64,
    pub lavadas: i64,
}

impl Jogador {
    fn da_linha(l: &sqlx::sqlite::SqliteRow) -> Jogador {
        Jogador {
            id: l.get("id"),
            apelido: l.get("apelido"),
            moedas: l.get("moedas"),
            partidas: l.get("partidas"),
            vitorias: l.get("vitorias"),
            derrotas: l.get("derrotas"),
            melhor_sequencia: l.get("melhor_sequencia"),
            lavadas: l.get("lavadas"),
        }
    }
}

// A lista de campos vai literal em cada consulta, repetida. Não é descuido: o sqlx 0.9
// recusa `query` com `String` montada em runtime (trait `SqlSafeStr`) justamente para que
// nenhuma consulta possa ser costurada com dado de fora. Repetir é o preço dessa guarda.

pub async fn criar_jogador(
    pool: &SqlitePool,
    apelido: &str,
    senha_hash: &str,
) -> Result<Jogador, sqlx::Error> {
    let id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO jogadores (id, apelido, senha_hash, moedas, criado_em)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&id)
    .bind(apelido)
    .bind(senha_hash)
    .bind(MOEDAS_INICIAIS)
    .bind(agora())
    .execute(pool)
    .await?;
    por_id(pool, &id).await.map(|o| o.expect("acabou de ser inserido"))
}

pub async fn por_id(pool: &SqlitePool, id: &str) -> Result<Option<Jogador>, sqlx::Error> {
    Ok(sqlx::query(
        "SELECT id, apelido, moedas, partidas, vitorias, derrotas, melhor_sequencia, lavadas
         FROM jogadores WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .map(|l| Jogador::da_linha(&l)))
}

/// Devolve a ficha e o hash da senha, para o `entrar`.
pub async fn por_apelido(
    pool: &SqlitePool,
    apelido: &str,
) -> Result<Option<(Jogador, String)>, sqlx::Error> {
    Ok(sqlx::query(
        "SELECT id, apelido, moedas, partidas, vitorias, derrotas, melhor_sequencia, lavadas,
                senha_hash
         FROM jogadores WHERE apelido = ?1 COLLATE NOCASE",
    )
        .bind(apelido)
        .fetch_optional(pool)
        .await?
        .map(|l| (Jogador::da_linha(&l), l.get("senha_hash"))))
}

pub async fn ranking(pool: &SqlitePool, teto: i64) -> Result<Vec<Jogador>, sqlx::Error> {
    // Vitórias primeiro, moedas como desempate: quem ganhou mais partidas está acima de
    // quem só apostou alto numa.
    let linhas = sqlx::query(
        "SELECT id, apelido, moedas, partidas, vitorias, derrotas, melhor_sequencia, lavadas
         FROM jogadores ORDER BY vitorias DESC, moedas DESC, apelido ASC LIMIT ?1",
    )
    .bind(teto)
    .fetch_all(pool)
    .await?;
    Ok(linhas.iter().map(Jogador::da_linha).collect())
}

/// Debita a aposta **se** o saldo cobrir. Devolve `false` sem tocar nada quando não cobre —
/// a condição está no `WHERE`, então a checagem e o débito são a mesma operação e não há
/// janela entre "conferi o saldo" e "cobrei".
pub async fn debitar(pool: &SqlitePool, id: &str, quanto: i64) -> Result<bool, sqlx::Error> {
    if quanto == 0 {
        return Ok(true);
    }
    let r = sqlx::query("UPDATE jogadores SET moedas = moedas - ?2 WHERE id = ?1 AND moedas >= ?2")
        .bind(id)
        .bind(quanto)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn creditar(pool: &SqlitePool, id: &str, quanto: i64) -> Result<(), sqlx::Error> {
    if quanto == 0 {
        return Ok(());
    }
    sqlx::query("UPDATE jogadores SET moedas = moedas + ?2 WHERE id = ?1")
        .bind(id)
        .bind(quanto)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Fecha a estatística de um jogador no fim de uma partida.
pub async fn registrar_resultado(
    pool: &SqlitePool,
    id: &str,
    ganhou: bool,
    lavada: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE jogadores SET
            partidas = partidas + 1,
            vitorias = vitorias + ?2,
            derrotas = derrotas + ?3,
            sequencia = CASE WHEN ?2 = 1 THEN sequencia + 1 ELSE 0 END,
            melhor_sequencia = MAX(melhor_sequencia,
                                   CASE WHEN ?2 = 1 THEN sequencia + 1 ELSE 0 END),
            lavadas = lavadas + ?4
         WHERE id = ?1",
    )
    .bind(id)
    .bind(i64::from(ganhou))
    .bind(i64::from(!ganhou))
    .bind(i64::from(ganhou && lavada))
    .execute(pool)
    .await
    .map(|_| ())
}

pub fn agora() -> String {
    jiff::Timestamp::now().to_string()
}
