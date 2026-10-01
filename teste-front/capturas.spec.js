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
const nome = (p) => `${p}${Date.now().toString(36)}${Math.floor(Math.random() * 1e4)}`;
const t = (p, chave) => p.locator(`[data-teste="${chave}"]`);

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
async function umPasso(paginas) {
  for (const p of paginas) {
    for (const b of ['btn-onze-aceitar', 'btn-aceitar']) {
      if (await t(p, b).isEnabled()) {
        await t(p, b).click();
        return true;
      }
    }
    const jogaveis = p.locator('[data-teste="carta"]:not([disabled])');
    if ((await jogaveis.count()) > 0) {
      await jogaveis.first().click();
      return true;
    }
  }
  return false;
}

/**
 * Joga uma partida 1x1 de verdade entre dois jogadores, para o ranking ter conteúdo.
 *
 * Recebe as páginas já abertas em vez de criar contexto: a primeira versão abria dois
 * contextos por partida e nunca os fechava a tempo, e o navegador morria.
 */
async function partidaDeVerdade(ps, a, b, aposta) {
  await entrar(ps[0], a);
  await entrar(ps[1], b);
  for (const p of ps) {
    await t(p, 'seletor-modo').selectOption('1x1');
    await t(p, 'campo-aposta').fill(String(aposta));
    await t(p, 'btn-procurar').click();
  }
  await expect(t(ps[0], 'mesa')).toBeVisible({ timeout: 30_000 });
  const prazo = Date.now() + 120_000;
  while (Date.now() < prazo) {
    if (await t(ps[0], 'fim').isVisible()) break;
    if (!(await umPasso(ps))) await ps[0].waitForTimeout(60);
  }
}

test('capturas do README', async ({ browser }) => {
  test.setTimeout(600_000);

  // Duas partidas de verdade primeiro: sem elas o ranking é uma lista de zeros.
  const ctxA = await browser.newContext();
  const ctxB = await browser.newContext();
  const par = [await ctxA.newPage(), await ctxB.newPage()];
  await partidaDeVerdade(par, nome('ana'), nome('bia'), 100);
  await partidaDeVerdade(par, nome('caio'), nome('duda'), 50);
  await ctxA.close();
  await ctxB.close();

  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  const p = await ctx.newPage();

  // 1. Entrada
  await p.goto('/');
  await p.waitForTimeout(400);
  await foto(p, '01-entrada.png');

  // 2. Saguão
  const eu = nome('voce');
  await entrar(p, eu);
  await p.waitForTimeout(400);
  await foto(p, '02-saguao.png');

  // 3. Ranking
  await p.locator('#aba-rank').click();
  await p.waitForTimeout(400);
  await foto(p, '03-ranking.png');

  // 4. Webhooks, com o segredo aparecendo pela única vez
  await p.locator('#aba-hooks').click();
  await t(p, 'campo-webhook-url').fill('https://example.com/truco');
  await t(p, 'btn-webhook-registrar').click();
  await expect(t(p, 'webhook-segredo')).toBeVisible({ timeout: 20_000 });
  await p.waitForTimeout(300);
  await foto(p, '04-webhooks.png');

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
  const prazoRodada = Date.now() + 60_000;
  while (Date.now() < prazoRodada) {
    if ((await p.locator('[data-teste="rodadas"] [data-carta]').count()) >= 2) break;
    if (!(await umPasso([p]))) await p.waitForTimeout(150);
  }
  await p.waitForTimeout(600);
  await foto(p, '07-rodadas.png');

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
  const ctx4 = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  const p4 = await ctx4.newPage();
  await entrar(p4, nome('duplas'));
  await t(p4, 'seletor-modo').selectOption('2x2');
  await t(p4, 'btn-treinar').click();
  await expect(t(p4, 'mesa')).toBeVisible({ timeout: 30_000 });
  await expect(t(p4, 'carta').first()).toBeVisible({ timeout: 20_000 });
  // Deixa uma rodada acontecer, para a mesa não estar vazia na foto.
  const prazo4 = Date.now() + 60_000;
  while (Date.now() < prazo4) {
    if ((await p4.locator('[data-teste="rodadas"] [data-carta]').count()) >= 3) break;
    if (!(await umPasso([p4]))) await p4.waitForTimeout(150);
  }
  await p4.waitForTimeout(700);
  await foto(p4, '10-mesa-2x2.png');
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
  const prazoC = Date.now() + 60_000;
  while (Date.now() < prazoC) {
    if ((await pc.locator('[data-teste="rodadas"] [data-carta]').count()) >= 2) break;
    if (!(await umPasso([pc]))) await pc.waitForTimeout(150);
  }
  await pc.waitForTimeout(600);
  await foto(pc, '12-celular-mesa.png');
  await cel.close();
});
