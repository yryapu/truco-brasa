# Protocolo

Duas superfícies: **HTTP** para o que é pedido-resposta (cadastro, saldo, ranking, webhooks)
e **WebSocket** para o que é partida. A regra é simples: se a resposta é para quem perguntou,
é HTTP; se o evento é para a mesa, é WebSocket.

A carta é sempre **o caractere Unicode**, uma string de um caractere: `"🂡"`. Nunca um objeto,
nunca um índice de baralho, nunca `"As de espadas"`. Carta de costas aparece como `null` com
`"coberta": true` — porque mandar `"🂠"` sugeriria que as costas são uma carta.

## HTTP

A sessão é um cookie `sessao` (`HttpOnly`, `SameSite=Strict`). Nenhuma rota aceita id de
jogador do cliente: quem você é vem sempre do cookie (ADR-002).

| rota | corpo | devolve |
|------|-------|---------|
| `POST /api/registrar` | `{"apelido":"...","senha":"..."}` | `{"id","apelido","moedas":1000}` + cookie. Já entra logado. |
| `POST /api/entrar` | idem | idem |
| `POST /api/sair` | — | `204`, cookie invalidado no servidor |
| `GET /api/eu` | — | `{"id","apelido","moedas","partidas","vitorias","derrotas","melhor_sequencia","emblemas":[...]}` |
| `GET /api/ranking` | — | lista ordenada por vitórias, depois moedas |
| `POST /api/webhooks` | `{"url":"https://..."}` | `{"id","url","segredo"}` — **o segredo aparece uma vez só** |
| `GET /api/webhooks` | — | lista sem o segredo |
| `DELETE /api/webhooks/{id}` | — | `204` |
| `GET /` | — | o cliente |

Erro é sempre `{"erro":"<código>","mensagem":"<em português>"}` com o status HTTP certo.
Códigos: `apelido_em_uso`, `credenciais_invalidas`, `nao_autenticado`, `apelido_invalido`,
`senha_curta`, `url_invalida`, `destino_proibido`, `limite_de_webhooks`, `aposta_invalida`.

## WebSocket

`GET /ws?modo=1x1|2x2&aposta=<n>` — autenticado pelo mesmo cookie; sem cookie, o *upgrade* é
recusado com `401`. `aposta` é em moedas, de `0` até o saldo. O pareamento junta jogadores com
o **mesmo** modo e a **mesma** aposta (ADR-007).

Toda mensagem tem o campo `t`. Do cliente para o servidor:

```json
{"t":"jogar","indice":0,"coberta":false}
{"t":"pedir"}
{"t":"aceitar"}
{"t":"correr"}
{"t":"aumentar"}
{"t":"onze","aceita":true}
```

`indice` é a posição em `minhas_cartas` do último `estado` recebido. Do servidor para o
cliente:

```json
{"t":"fila","modo":"2x2","aposta":50,"faltam":3}
{"t":"mesa","partida":"<uuid>","assento":1,"aposta":50,
 "jogadores":[{"assento":0,"apelido":"ana","equipe":0}, …]}
{"t":"estado", … a visão deste assento, abaixo … }
{"t":"avisos","avisos":[{"aviso":"jogou","assento":0,"carta":"🂡"}, …]}
{"t":"erro","erro":"nao_eh_sua_vez","mensagem":"não é a sua vez"}
{"t":"fim","vencedora":0,"placar":[12,7],"moedas":1050,"ganho":50}
```

### `estado` — a visão de **um** assento

```json
{"t":"estado","assento":1,"equipe":1,"modo":"dois_contra_dois",
 "placar":[3,6],"numero_da_mao":4,
 "vira":"🃅","manilha":"🃖",
 "minhas_cartas":["🂡","🃝","4🃔"],
 "cartas_do_parceiro":null,
 "mesa":[{"assento":0,"carta":"🂣","coberta":false},
         {"assento":3,"carta":null,"coberta":true}],
 "rodadas":[0,null],
 "cartas_na_mao":[2,3,3,2],
 "vez":1,"valor":3,"tipo":"normal","pendencia":null,
 "aguarda_onze":false,"vencedora":null,
 "acoes":["jogar","jogar_coberta"]}
```

O invariante que o teste prova (`visao_nunca_vaza`): **nenhum caractere de carta que apareça
em `estado` para o assento `i` pertence à mão de outro assento**, e carta coberta não traz
`carta`. A única exceção é `cartas_do_parceiro`, preenchido só na mão de onze da própria
dupla (R-25).

`acoes` existe para a interface desenhar botões. **Não é autoridade**: o servidor revalida
tudo em `aplicar`. Se as duas discordarem, quem vale é `aplicar`.

`rodadas` traz a dupla vencedora de cada rodada já resolvida; `null` dentro da lista é rodada
empatada.

### O nome do pedido

O rótulo vem do **valor proposto**, nunca do valor atual da mão:

| `valor_proposto` | nome |
|---|---|
| 3 | TRUCO |
| 6 | SEIS |
| 9 | NOVE |
| 12 | DOZE |

Logo o botão de pedir, com a mão valendo 1, diz "TRUCO!" (propõe 3); com a mão em 3, diz
"SEIS!". E o de aumentar segue `pendencia.valor_proposto`: se alguém pediu TRUCO
(`valor_proposto: 3`), aumentar propõe 6, então o botão diz "SEIS!".

Isto está escrito porque eu descrevi a tabela de um jeito e dei um exemplo contraditório no
mesmo texto. Quem construiu o cliente achou a contradição antes de mim, e escolheu a tabela.

## Webhooks

`POST` na URL registrada, corpo JSON, com:

```
X-Truco-Evento:     partida.comecou | partida.terminou
X-Truco-Entrega:    <uuid, único por tentativa>
X-Truco-Assinatura: sha256=<hex do HMAC-SHA256 do corpo exato, com o segredo>
```

```json
{"evento":"partida.terminou","em":"2026-10-01T12:00:00Z",
 "partida":{"id":"<uuid>","modo":"2x2","aposta":50},
 "resultado":{"equipe_vencedora":0,"placar":[12,7],
              "vencedores":["ana","bia"],"perdedores":["caio","duda"],
              "moedas_por_vencedor":100}}
```

O "resultado" que o enunciado pede é **o corpo de `partida.terminou`**, não um terceiro
evento: um evento `resultado` vazio seria camada sem capacidade (ADR-004).

Verificação, do lado do integrador:

```sh
printf '%s' "$CORPO" | openssl dgst -sha256 -hmac "$SEGREDO" -hex
```
