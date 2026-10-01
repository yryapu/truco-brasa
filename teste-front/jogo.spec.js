// Teste de interface: dois navegadores independentes se encontram numa mesa e jogam uma
// partida inteira. Cada `context` tem cookies próprios, que é a forma exata do problema —
// duas sessões, dois jogadores (ADR-008).

const { test, expect } = require('@playwright/test');

/**
 * Abre um contexto e **garante que ele fecha** no fim do teste.
 *
 * `browser.newContext()` não é fechado por teste — ele vive até o navegador morrer. Sem
 * isto, ao chegar no quinto teste havia seis páginas abertas, cada uma com um WebSocket
 * vivo e as animações da mesa rodando; num Chromium headless dentro de container isso
 * estrangula a CPU, e o teste que joga uma partida inteira pela interface passava de 7 s
 * para mais de 4 min. O sintoma era "estourou o tempo" no último teste, o que faz parecer
 * defeito do que ele testa — e não era: era deste arquivo.
 */
async function contexto(browser) {
  const ctx = await browser.newContext();
  contextosAbertos.push(ctx);
  return ctx;
}
let contextosAbertos = [];
test.afterEach(async () => {
  await Promise.all(contextosAbertos.map((c) => c.close().catch(() => {})));
  contextosAbertos = [];
});

/** Apelido único por execução: o banco do container sobrevive entre testes. */
const nome = (p) => `${p}${Date.now().toString(36)}${Math.floor(Math.random() * 1e4)}`;

const t = (p, chave) => p.locator(`[data-teste="${chave}"]`);

/** Cria conta e já cai no saguão — é o que o ADR-002 promete: dois campos, sem confirmação. */
async function entrar(pagina, apelido) {
  await pagina.goto('/');
  await t(pagina, 'campo-apelido').fill(apelido);
  await t(pagina, 'campo-senha').fill('senha-de-teste-1');
  await t(pagina, 'btn-registrar').click();
  await expect(t(pagina, 'saldo')).toHaveText('1000', { timeout: 20_000 });
}

async function procurar(pagina, modo, aposta) {
  await t(pagina, 'seletor-modo').selectOption(modo);
  await t(pagina, 'campo-aposta').fill(String(aposta));
  await t(pagina, 'btn-procurar').click();
}

/**
 * As cartas da própria mão, como o **caractere Unicode** que trafega.
 *
 * Lê `data-carta` e não o texto: o cliente desenha a face da carta (valor nos cantos, naipe
 * no centro), então o `textContent` é "A♠A♠" e não `🂡`. O caractere continua sendo o dado —
 * fica em `data-carta` e no `title` exatamente para quem inspeciona poder conferir.
 */
async function minhaMao(pagina) {
  return t(pagina, 'carta').evaluateAll((es) =>
    es.map((e) => e.getAttribute('data-carta')),
  );
}

/** É caractere do bloco Playing Cards, e não um 8/9/10 nem o Cavaleiro (que o truco não tem). */
function ehCartaDoTruco(ch) {
  const cp = ch.codePointAt(0);
  const base = cp & ~0xf;
  const baixo = cp & 0xf;
  const naipeOk = [0x1f0a0, 0x1f0b0, 0x1f0c0, 0x1f0d0].includes(base);
  const numeroOk = [1, 2, 3, 4, 5, 6, 7, 0xb, 0xd, 0xe].includes(baixo);
  return naipeOk && numeroOk;
}

/**
 * Faz uma jogada em qualquer página que tenha ação disponível. Aceita todo truco e nunca
 * aumenta: o objetivo é provar que a partida fecha pela interface, não jogar bem.
 * Devolve `true` se agiu em alguma página.
 */
async function umPasso(paginas) {
  for (const p of paginas) {
    if (await t(p, 'btn-onze-aceitar').isEnabled()) {
      await t(p, 'btn-onze-aceitar').click();
      return true;
    }
    if (await t(p, 'btn-aceitar').isEnabled()) {
      await t(p, 'btn-aceitar').click();
      return true;
    }
    const jogaveis = p.locator('[data-teste="carta"]:not([disabled])');
    if ((await jogaveis.count()) > 0) {
      await jogaveis.first().click();
      return true;
    }
  }
  return false;
}

test('1x1: dois navegadores se encontram, jogam até 12, e o saldo fecha', async ({ browser }) => {
  const [ca, cb] = [await contexto(browser), await contexto(browser)];
  const [pa, pb] = [await ca.newPage(), await cb.newPage()];
  const [ana, bia] = [nome('ana'), nome('bia')];

  await entrar(pa, ana);
  await entrar(pb, bia);

  const aposta = 100;
  await procurar(pa, '1x1', aposta);
  await procurar(pb, '1x1', aposta);

  // A mesa apareceu para os dois.
  await expect(t(pa, 'mesa')).toBeVisible({ timeout: 30_000 });
  await expect(t(pb, 'mesa')).toBeVisible({ timeout: 30_000 });

  // Três cartas cada, e cada uma é um caractere Unicode de carta do truco.
  const maoA = await minhaMao(pa);
  const maoB = await minhaMao(pb);
  expect(maoA).toHaveLength(3);
  expect(maoB).toHaveLength(3);
  for (const c of [...maoA, ...maoB]) {
    expect([...c]).toHaveLength(1);
    expect(ehCartaDoTruco(c), `${c} deveria ser carta do baralho de 40`).toBe(true);
  }

  // Isolamento visto pelo navegador: nenhuma carta da ana está na tela da bia.
  const telaB = await pb.locator('body').innerText();
  for (const c of maoA) {
    expect(telaB.includes(c), `a tela da bia mostrou ${c}, que é da ana`).toBe(false);
  }
  const telaA = await pa.locator('body').innerText();
  for (const c of maoB) {
    expect(telaA.includes(c), `a tela da ana mostrou ${c}, que é da bia`).toBe(false);
  }

  // Joga até alguém chegar a 12.
  const paginas = [pa, pb];
  for (let i = 0; i < 1200; i++) {
    const acabou =
      (await t(pa, 'fim').isVisible()) || (await t(pb, 'fim').isVisible());
    if (acabou) break;
    if (!(await umPasso(paginas))) await pa.waitForTimeout(80);
  }

  await expect(t(pa, 'fim')).toBeVisible({ timeout: 30_000 });
  await expect(t(pb, 'fim')).toBeVisible({ timeout: 30_000 });

  // Um ganhou 100, o outro perdeu 100, e as duas telas contam a mesma história.
  const textoA = await t(pa, 'fim').innerText();
  const textoB = await t(pb, 'fim').innerText();
  const venceu = (s) => /vitória/i.test(s);
  expect(venceu(textoA)).not.toBe(venceu(textoB));

  // E o saldo no servidor fecha: 2000 no total, 1100 e 900.
  const saldo = async (p) => {
    const r = await p.request.get('/api/eu');
    return (await r.json()).moedas;
  };
  const saldos = [await saldo(pa), await saldo(pb)].sort((x, y) => x - y);
  expect(saldos).toEqual([900, 1100]);
});

test('o saguão mostra emblema, ranking e webhook, e o segredo aparece uma vez', async ({
  browser,
}) => {
  const ctx = await contexto(browser);
  const p = await ctx.newPage();
  const quem = nome('caio');
  await entrar(p, quem);

  // Estreante, porque ainda não terminou partida.
  await expect(t(p, 'emblemas')).toContainText('Estreante');

  // O ranking é público e lista quem já jogou.
  await expect(t(p, 'ranking')).toBeVisible();

  // Webhook: registrar devolve o segredo, e ele aparece na tela uma vez.
  await p.locator('#aba-hooks').click();
  // `example.com` e não `exemplo.invalid`: a guarda de destino resolve o host e recusa o
  // que não resolve (ADR-004), então um domínio inexistente é legitimamente rejeitado — foi
  // o que a primeira execução deste teste mostrou. `example.com` é reservado pela IANA para
  // exemplos, resolve, é público, e nunca recebe entrega porque nenhuma partida acontece
  // aqui. O custo é que este teste precisa de DNS.
  await t(p, 'campo-webhook-url').fill('https://example.com/entrega');
  await t(p, 'btn-webhook-registrar').click();
  const segredo = t(p, 'webhook-segredo');
  await expect(segredo).toBeVisible({ timeout: 20_000 });
  await expect(segredo).toHaveText(/^[0-9a-f]{64}$/);

  // Recarregar a página não traz o segredo de volta — ele não é guardado em lugar nenhum
  // que o cliente possa ler.
  await p.reload();
  await expect(t(p, 'saldo')).toHaveText('1000');
  const lista = await (await p.request.get('/api/webhooks')).json();
  expect(lista).toHaveLength(1);
  expect(lista[0].segredo).toBeUndefined();
});

test('destino proibido é recusado pela interface com mensagem em português', async ({
  browser,
}) => {
  const ctx = await contexto(browser);
  const p = await ctx.newPage();
  await entrar(p, nome('duda'));
  await p.locator('#aba-hooks').click();
  await t(p, 'campo-webhook-url').fill('http://169.254.169.254/latest/meta-data/');
  await t(p, 'btn-webhook-registrar').click();
  await expect(p.locator('body')).toContainText(/não é permitido|não é uma URL/i, {
    timeout: 20_000,
  });
});

test('do primeiro clique à mesa em menos de um minuto', async ({ browser }) => {
  // O enunciado pede "o jogador entra e começa a jogar em menos de um minuto". Isto é o
  // critério que eu marcaria como atendido sem verificar, então é o que precisa de número.
  // Mede duas pessoas, do `goto` até a primeira carta na mão — não só o cadastro.
  const t0 = Date.now();
  const [ca, cb] = [await contexto(browser), await contexto(browser)];
  const [pa, pb] = [await ca.newPage(), await cb.newPage()];

  await Promise.all([entrar(pa, nome('rapido-a')), entrar(pb, nome('rapido-b'))]);
  const apósCadastro = Date.now() - t0;

  await procurar(pa, '1x1', 0);
  await procurar(pb, '1x1', 0);
  await expect(t(pa, 'carta').first()).toBeVisible({ timeout: 30_000 });
  await expect(t(pb, 'carta').first()).toBeVisible({ timeout: 30_000 });
  const atéJogar = Date.now() - t0;

  console.log(
    `[medida] cadastro de dois jogadores: ${apósCadastro} ms · ` +
      `até a carta na mão: ${atéJogar} ms`,
  );
  expect(atéJogar).toBeLessThan(60_000);
});

test('treino contra bot: jogo sozinho até o fim, e não vale moeda nem ranking', async ({
  browser,
}) => {
  // Mais folgado que o padrão: a partida inteira é jogada pela interface, e cada jogada do
  // bot passa pela rede mais a pausa dele.
  test.setTimeout(240_000);
  // É o caminho para quem quer jogar sem ter com quem, e provavelmente o mais usado.
  const ctx = await contexto(browser);
  const p = await ctx.newPage();
  await entrar(p, nome('sozinho'));

  await t(p, 'seletor-modo').selectOption('1x1');
  await t(p, 'btn-treinar').click();

  await expect(t(p, 'mesa')).toBeVisible({ timeout: 30_000 });
  // O selo de treino tem de ser visível na mesa, não só um aviso que passa.
  await expect(p.locator('body')).toContainText(/treino/i);
  // Três cartas, e o adversário é um bot (nome na mesa).
  await expect(t(p, 'carta')).toHaveCount(3);

  // Joga até o fim. Só eu clico — o bot se move sozinho, então cada iteração espera um
  // pouco se não houver nada habilitado.
  // Laço com prazo de parede, não contagem de iterações: contado por iteração, o teto do
  // próprio teste estourava antes e o diagnóstico abaixo nunca rodava.
  const prazo = Date.now() + 90_000;
  let acabou = false;
  while (Date.now() < prazo) {
    if (await t(p, 'fim').isVisible()) {
      acabou = true;
      break;
    }
    if (!(await umPasso([p]))) await p.waitForTimeout(120);
  }
  if (!acabou) {
    // Diagnóstico: sem isto, o único sintoma é "estourou o tempo", que não diz nada.
    const erro = await t(p, 'erro').innerText().catch(() => '(sem erro)');
    const log = await t(p, 'log').innerText().catch(() => '(sem log)');
    const habilitadas = await p.locator('[data-teste="carta"]:not([disabled])').count();
    const placar = await t(p, 'placar').innerText().catch(() => '(sem placar)');
    console.log('[diag] erro:', JSON.stringify(erro));
    console.log('[diag] placar:', JSON.stringify(placar));
    console.log('[diag] cartas habilitadas:', habilitadas);
    console.log('[diag] últimas linhas do log:', JSON.stringify(log.split('\n').slice(-8)));
  }
  await expect(t(p, 'fim')).toBeVisible({ timeout: 30_000 });

  // Nada contou: saldo intacto, nenhuma partida registrada, ainda estreante.
  const eu = await (await p.request.get('/api/eu')).json();
  expect(eu.moedas).toBe(1000);
  expect(eu.partidas).toBe(0);
  expect(eu.emblemas.map((e) => e.chave)).toContain('estreante');
});
