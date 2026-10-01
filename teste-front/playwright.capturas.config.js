// Configuração só das capturas. Separada porque a suíte as ignora: foto não afirma nada,
// e um arquivo que não afirma nada não deve poder reprovar o CI.
module.exports = {
  testDir: '.',
  testMatch: 'capturas.spec.js',
  timeout: 600_000,
  expect: { timeout: 30_000 },
  // SEM isto, um clique espera para sempre. O padrão do Playwright para ação é "sem
  // limite", e a mesa é redesenhada inteira a cada `estado` — então um clique numa carta
  // pode pegar o elemento sendo destruído, entrar em "element was detached, retrying" e
  // nunca sair. Foi o que travou a primeira captura por dez minutos.
  workers: 1,
  reporter: [['list']],
  use: {
    actionTimeout: 8_000,
    baseURL: process.env.BASE || 'http://127.0.0.1:8080',
    // Escala 1: o GitHub mostra a imagem com uns 800 px de largura, então 1280 já é nítido
    // e o repositório não ganha megabytes de retina que ninguém vê.
    deviceScaleFactor: 1,
    launchOptions: {
      // Cinto e suspensório junto do `shm_size` do compose: com esta bandeira o Chromium
      // usa /tmp em vez de /dev/shm, então a captura não depende de o compose estar certo.
      args: ['--disable-dev-shm-usage'],
    },
  },
};
