// Teste de interface: dois navegadores independentes se encontram numa mesa e jogam uma
// partida inteira. Cada `context` tem cookies próprios, que é a forma exata do problema —
// duas sessões, dois jogadores (ADR-008).

const { test, expect } = require('@playwright/test');

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

/** As cartas da própria mão, como texto. */
async function minhaMao(pagina) {
  return (await t(pagina, 'carta').allTextContents()).map((s) => s.trim());
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
  const [ca, cb] = [await browser.newContext(), await browser.newContext()];
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
  const ctx = await browser.newContext();
  const p = await ctx.newPage();
  const quem = nome('caio');
  await entrar(p, quem);

  // Estreante, porque ainda não terminou partida.
  await expect(t(p, 'emblemas')).toContainText('Estreante');

  // O ranking é público e lista quem já jogou.
  await expect(t(p, 'ranking')).toBeVisible();

  // Webhook: registrar devolve o segredo, e ele aparece na tela uma vez.
  await p.locator('#aba-hooks').click();
  await t(p, 'campo-webhook-url').fill('https://exemplo.invalid/entrega');
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
  const ctx = await browser.newContext();
  const p = await ctx.newPage();
  await entrar(p, nome('duda'));
  await p.locator('#aba-hooks').click();
  await t(p, 'campo-webhook-url').fill('http://169.254.169.254/latest/meta-data/');
  await t(p, 'btn-webhook-registrar').click();
  await expect(p.locator('body')).toContainText(/não é permitido|não é uma URL/i, {
    timeout: 20_000,
  });
});
