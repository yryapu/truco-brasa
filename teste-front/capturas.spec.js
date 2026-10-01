// Capturas de tela para o README. Não é teste: não afirma nada, só fotografa.
//
// Roda com o bot propositalmente lento (TRUCO_PAUSA_BOT_MS alto), porque alguns momentos
// que importam — o truco esperando resposta — duram menos que um piscar com o bot normal.
//
// Fica fora da suíte por `testIgnore` em playwright.config.js, e roda pelo
// playwright.capturas.config.js.

const { test, expect } = require('@playwright/test');
const fs = require('fs');

const PASTA = '/teste/capturas';
// Nomes fixos e legíveis: o volume é apagado antes de cada captura (`down -v`), então não
// há colisão — e apelido com sufixo aleatório polui a foto que o README vai mostrar.
const nome = (p) => p;
const t = (p, chave) => p.locator(`[data-teste="${chave}"]`);

/**
 * Fotografa a mesa **no instante** em que uma rodada fecha com `quantas` cartas à vista.
 *
 * A foto é tirada aqui dentro, assim que a condição vale e é reconfirmada — e não por quem
 * chama, depois. Duas versões anteriores separavam "esperar" de "fotografar", e as duas
 * fotografaram a mesa vazia: entre o `return` e o `screenshot` a mão virava.
 *
 * Também não basta esperar "aparecerem cartas nas faixas": isso acontece ao fim de qualquer
 * rodada, inclusive a que **encerra** a mão — e aí as faixas zeram logo em seguida. Por isso
 * a espera começa numa mão recém-distribuída (faixas vazias) e joga **uma** carta: a rodada
 * 1 nunca encerra a mão, porque uma mão precisa de pelo menos duas rodadas decididas.
 */
async function fotoDeRodadaFechada(p, arquivo, quantas) {
  const naRodada = p.locator('[data-teste="rodadas"] [data-carta]');
  for (let tentativa = 1; tentativa <= 4; tentativa++) {
    const prazo = Date.now() + 60_000;
    let joguei = false;
    while (Date.now() < prazo && !joguei) {
      if ((await naRodada.count()) === 0) {
        const jogaveis = p.locator('[data-teste="carta"]:not([disabled])');
        if ((await jogaveis.count()) > 0) {
          try {
            await jogaveis.first().click({ timeout: 4_000 });
            joguei = true;
            break;
          } catch {
            /* redesenho no meio: tenta de novo */
          }
        }
      } else if (!(await umPasso([p]))) {
        await p.waitForTimeout(150);
      }
      await p.waitForTimeout(100);
    }
    if (!joguei) {
      console.log(`[foto] tentativa ${tentativa} de ${arquivo}: não consegui jogar numa mão nova`);
      continue;
    }
    // Agora ninguém mais joga por mim: só se espera os outros assentos fecharem a rodada.
    const ate = Date.now() + 45_000;
    while (Date.now() < ate) {
      if ((await naRodada.count()) >= quantas) {
        await p.waitForTimeout(250); // assenta a animação
        const aindaLa = await naRodada.count();
        if (aindaLa >= quantas) {
          await foto(p, arquivo);
          return true;
        }
        console.log(`[foto] ${arquivo}: as faixas zeraram entre a condição e a foto (${aindaLa})`);
        break;
      }
      await p.waitForTimeout(150);
    }
    console.log(`[foto] tentativa ${tentativa} de ${arquivo}: a rodada não fechou a tempo`);
  }
  console.log(`[foto] DESISTI de ${arquivo} — fotografando como estiver`);
  await foto(p, arquivo);
  return false;
}

async function foto(pagina, arquivo, opcoes = {}) {
  fs.mkdirSync(PASTA, { recursive: true });
  await pagina.screenshot({ path: `${PASTA}/${arquivo}`, ...opcoes });
  console.log(`[foto] ${arquivo}`);
}

async function entrar(pagina, apelido) {
  await pagina.goto('/');
  await t(pagina, 'campo-apelido').fill(apelido);
  await t(pagina, 'campo-senha').fill('senha-de-teste-1');
  await t(pagina, 'btn-registrar').click();
  await expect(t(pagina, 'saldo')).toHaveText('1000', { timeout: 20_000 });
}

/** Uma jogada em qualquer página que tenha ação. Igual ao da suíte. */
/**
 * Uma jogada em qualquer página que tenha ação.
 *
 * Todo clique tem prazo e é tolerante a falha: a mesa é redesenhada inteira a cada `estado`,
 * então um clique pode pegar o elemento sendo destruído. Aqui isso é ruído a repetir na
 * volta do laço, nunca motivo para parar — e nunca espera infinita.
 */
async function umPasso(paginas) {
  for (const p of paginas) {
    for (const b of ['btn-onze-aceitar', 'btn-aceitar']) {
      try {
        if (await t(p, b).isEnabled({ timeout: 2_000 })) {
          await t(p, b).click({ timeout: 4_000 });
          return true;
        }
      } catch {
        /* redesenho no meio do caminho: tenta de novo na volta do laço */
      }
    }
    try {
      const jogaveis = p.locator('[data-teste="carta"]:not([disabled])');
      if ((await jogaveis.count()) > 0) {
        await jogaveis.first().click({ timeout: 4_000 });
        return true;
      }
    } catch {
      /* idem */
    }
  }
  return false;
}

test('capturas do README', async ({ browser }) => {
  test.setTimeout(600_000);

  // O ranking já vem semeado por `semear.js`, que joga as partidas de verdade por WebSocket
  // cru, sem navegador. Aqui só se fotografa.

  const ctx = await browser.newContext({ viewport: { width: 1280, height: 1000 } });
  const p = await ctx.newPage();

  // 1. Entrada
  await p.goto('/');
  await p.waitForTimeout(400);
  await foto(p, '01-entrada.png');

  // 2. Saguão
  const eu = nome('voce');
  await entrar(p, eu);
  await p.waitForTimeout(400);
  await foto(p, '02-saguao.png', { fullPage: true });

  // 3. Ranking
  await p.locator('#aba-rank').click();
  await p.waitForTimeout(400);
  await foto(p, '03-ranking.png', { fullPage: true });

  // 4. Webhooks, com o segredo aparecendo pela única vez
  await p.locator('#aba-hooks').click();
  await t(p, 'campo-webhook-url').fill('https://example.com/truco');
  await t(p, 'btn-webhook-registrar').click();
  await expect(t(p, 'webhook-segredo')).toBeVisible({ timeout: 20_000 });
  await p.waitForTimeout(300);
  await foto(p, '04-webhooks.png', { fullPage: true });

  // 5. Mesa de treino 1x1
  await p.locator('#aba-rank').click();
  await t(p, 'seletor-modo').selectOption('1x1');
  await t(p, 'btn-treinar').click();
  await expect(t(p, 'mesa')).toBeVisible({ timeout: 30_000 });
  await expect(t(p, 'carta').first()).toBeVisible({ timeout: 20_000 });
  await p.waitForTimeout(600);
  await foto(p, '05-mesa-1x1.png');

  // 6. Truco pendente: pedir e fotografar antes de o bot responder (ele está lento)
  if (await t(p, 'btn-pedir').isEnabled()) {
    await t(p, 'btn-pedir').click();
    await p.waitForTimeout(500);
    await foto(p, '06-truco.png');
  } else {
    // Joga uma carta e tenta de novo na rodada seguinte.
    await p.locator('[data-teste="carta"]:not([disabled])').first().click();
    await p.waitForTimeout(3_000);
    if (await t(p, 'btn-pedir').isEnabled()) {
      await t(p, 'btn-pedir').click();
      await p.waitForTimeout(500);
      await foto(p, '06-truco.png');
    }
  }

  // 7. Rodada resolvida continuando em tela
  await fotoDeRodadaFechada(p, '07-rodadas.png', 2);

  // 8. Histórico, com pelo menos duas mãos e os filtros à vista
  const prazoHist = Date.now() + 180_000;
  while (Date.now() < prazoHist) {
    const rotulo = (await t(p, 'btn-historico').innerText().catch(() => '')) || '';
    if (Number((rotulo.match(/\d+/) || [0])[0]) >= 2) break;
    if (await t(p, 'fim').isVisible()) break;
    if (!(await umPasso([p]))) await p.waitForTimeout(120);
  }
  await t(p, 'btn-historico').click();
  await expect(t(p, 'historico')).toBeVisible();
  await p.waitForTimeout(500);
  await foto(p, '08-historico.png');
  await p.keyboard.press('Escape');

  // 9. Fim de partida
  const prazoFim = Date.now() + 240_000;
  while (Date.now() < prazoFim) {
    if (await t(p, 'fim').isVisible()) break;
    if (!(await umPasso([p]))) await p.waitForTimeout(120);
  }
  await p.waitForTimeout(600);
  await foto(p, '09-fim.png');
  await ctx.close();

  // 10. Mesa 2x2 de treino: você, o parceiro e dois adversários
  const ctx4 = await browser.newContext({ viewport: { width: 1280, height: 1000 } });
  const p4 = await ctx4.newPage();
  await entrar(p4, nome('duplas'));
  await t(p4, 'seletor-modo').selectOption('2x2');
  await t(p4, 'btn-treinar').click();
  await expect(t(p4, 'mesa')).toBeVisible({ timeout: 30_000 });
  await expect(t(p4, 'carta').first()).toBeVisible({ timeout: 20_000 });
  // Uma rodada inteira dos quatro, para a foto mostrar o que a mesa de duplas tem de
  // próprio: as quatro cartas, de quem é cada uma, e qual delas levou.
  await fotoDeRodadaFechada(p4, '10-mesa-2x2.png', 4);
  await ctx4.close();

  // 11. Celular: a mesa em 390 px
  const cel = await browser.newContext({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
  });
  const pc = await cel.newPage();
  await entrar(pc, nome('celular'));
  await foto(pc, '11-celular-saguao.png');
  await t(pc, 'seletor-modo').selectOption('1x1');
  await t(pc, 'btn-treinar').click();
  await expect(t(pc, 'mesa')).toBeVisible({ timeout: 30_000 });
  await expect(t(pc, 'carta').first()).toBeVisible({ timeout: 20_000 });
  await fotoDeRodadaFechada(pc, '12-celular-mesa.png', 2);
  await cel.close();
});
