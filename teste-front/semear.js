// Semeia o servidor com partidas de verdade, para o ranking das fotos não ser uma lista de
// zeros. Node puro com WebSocket cru — **sem navegador**.
//
// A primeira versão fazia isso com dois Chromium jogando a partida inteira pela interface, e
// caía com "Target crashed": numa máquina com uma dúzia de containers, dois navegadores
// jogando doze pontos é peso que não compra nada. Semear não precisa de interface; fotografar
// precisa. São trabalhos diferentes e agora são processos diferentes.

const WebSocket = require('ws');

const BASE = process.env.BASE || 'http://127.0.0.1:8080';
const SENHA = 'senha-de-teste-1';

async function registrar(apelido) {
  const r = await fetch(`${BASE}/api/registrar`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ apelido, senha: SENHA }),
  });
  if (!r.ok) throw new Error(`registrar ${apelido}: ${r.status} ${await r.text()}`);
  const cookie = (r.headers.getSetCookie?.() || [r.headers.get('set-cookie')])
    .filter(Boolean)
    .map((c) => c.split(';')[0])
    .join('; ');
  return cookie;
}

/**
 * Joga uma partida inteira. Aceita todo truco, nunca aumenta, sempre põe a primeira carta:
 * o objetivo é gerar resultado, não jogar bem.
 */
function jogar(cookie, modo, aposta) {
  return new Promise((resolve, reject) => {
    const url = `${BASE.replace('http', 'ws')}/ws?modo=${modo}&aposta=${aposta}`;
    const ws = new WebSocket(url, { headers: { Cookie: cookie } });
    const prazo = setTimeout(() => {
      ws.close();
      reject(new Error('partida não terminou em 60s'));
    }, 60_000);

    ws.on('message', (dados) => {
      const m = JSON.parse(dados.toString());
      if (m.t === 'fim') {
        clearTimeout(prazo);
        ws.close();
        resolve(m);
        return;
      }
      if (m.t !== 'estado') return;
      const acoes = m.acoes || [];
      if (acoes.includes('onze_aceitar')) ws.send(JSON.stringify({ t: 'onze', aceita: true }));
      else if (acoes.includes('aceitar')) ws.send(JSON.stringify({ t: 'aceitar' }));
      else if (acoes.includes('jogar'))
        ws.send(JSON.stringify({ t: 'jogar', indice: 0, coberta: false }));
    });
    ws.on('error', (e) => {
      clearTimeout(prazo);
      reject(e);
    });
  });
}

async function partida(a, b, aposta) {
  const [ca, cb] = [await registrar(a), await registrar(b)];
  const [fa] = await Promise.all([jogar(ca, '1x1', aposta), jogar(cb, '1x1', aposta)]);
  console.log(`[semear] ${a} x ${b} por ${aposta}: dupla ${fa.vencedora} levou, ${fa.placar}`);
}

(async () => {
  // Sem sufixo: o volume do banco é apagado antes de cada captura, então não há colisão —
  // e nome com lixo aleatório estraga a foto do ranking que vai para o README.
  // Quatro partidas: o ranking das fotos mostra gente com vitórias, derrotas e saldos
  // diferentes, em vez de uma coluna de zeros.
  await partida('ana', 'bia', 100);
  await partida('caio', 'duda', 50);
  await partida('zeca', 'nina', 200);
  await partida('lurdes', 'bento', 10);
  console.log('[semear] pronto');
})().catch((e) => {
  console.error('[semear] falhou:', e.message);
  process.exit(1);
});
