# truco-brasa

Truco paulista jogável na web, 1x1 e 2x2. Servidor em Rust. A carta trafega no WebSocket como
**o próprio caractere Unicode** (`🂡 🂱 🃁 🃑`).

**Pesquisa, fontes e decisões:** https://github.com/yryapu/poliorketikos-truco-brasa

Cada regra de jogo implementada aqui cita a etiqueta (`R-01`…`R-31`) da especificação
normativa que vive no repositório de pesquisa, junto da fonte que a sustenta. Teste sem
etiqueta é teste de infraestrutura; regra sem etiqueta seria regra sem procedência.

## Como rodar

```sh
# tudo de pé, na porta 8080
docker compose up --build

# ou sem Docker
cargo run --release --bin truco      # http://127.0.0.1:8080
```

Abra `http://127.0.0.1:8080`, escolha um apelido e uma senha de 8 caracteres, e jogue. Para
ver uma partida, abra a mesma página em duas janelas (ou em duas pessoas) com a **mesma
aposta** e o mesmo modo — o pareamento junta quem escolheu o mesmo par (ADR-007).

## Como testar

```sh
cargo test --all        # regras e servidor: 46 testes, nenhum mock
docker compose -f compose.teste.yaml up --abort-on-container-exit --exit-code-from front
```

O segundo sobe o servidor em container e roda o Playwright contra ele, com dois navegadores
independentes numa mesma mesa. Ver `ADR-008` para por que Playwright e por que em container.

## Variáveis de ambiente

| nome | padrão | o que faz |
|------|--------|-----------|
| `TRUCO_ENDERECO` | `127.0.0.1:8080` | onde escutar |
| `TRUCO_BANCO` | `sqlite://truco.db` | arquivo do SQLite |
| `TRUCO_SEGURO` | desligado | liga `Secure` no cookie e exige `https` no webhook. **Ligue em produção.** |
| `TRUCO_WEBHOOK_LOCAL` | desligado | deixa o webhook apontar para endereço privado. Só para testar na própria máquina; o servidor recusa subir com isto junto de `TRUCO_SEGURO=1`. |

## Mapa

| onde | o que |
|------|-------|
| `crates/regras` | as regras do truco, puras: sem rede, sem banco, sem async. Testadas regra por regra |
| `crates/servidor` | axum: HTTP, WebSocket, sessão, saldo, mesas, ranking, webhooks |
| `cliente/index.html` | o cliente inteiro, um arquivo, sem build. Vai embutido no binário |
| `teste-front/` | Playwright |
| `PROTOCOLO.md` | o contrato entre cliente e servidor |

## Estado

v1. Ver `resultado.json` na raiz para o que está provado, com o comando que prova, e para o
que **não** está — que ficou em `riscos_conhecidos` em vez de na lista de feitos.
