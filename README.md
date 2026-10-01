# truco-brasa

Truco paulista jogável na web, 1x1 e 2x2. Servidor em Rust. A carta trafega no WebSocket como
**o próprio caractere Unicode** (`🂡 🂱 🃁 🃑`).

**Pesquisa, fontes e decisões:** https://github.com/yryapu/poliorketikos-truco-brasa

Cada regra de jogo implementada aqui cita a etiqueta (`R-01`…`R-31`) da especificação
normativa que vive no repositório de pesquisa, junto da fonte que a sustenta. Teste sem
etiqueta é teste de infraestrutura; regra sem etiqueta seria regra sem procedência.

## Estado

Em construção, aberto. Ver commits.

- [x] motor de regras (`crates/regras`) — baralho, força, manilha, Unicode
- [ ] máquina de estados da mão e da partida
- [ ] servidor (axum): cadastro, sessão, saldo, mesas, WebSocket
- [ ] ranking e emblemas
- [ ] webhooks
- [ ] cliente web
- [ ] Docker e testes de ponta a ponta
