// Configuração mínima. A base vem do ambiente para que o mesmo arquivo rode contra o
// container (`http://truco:8080`, nome do serviço no compose) ou contra um servidor local
// (`BASE=http://127.0.0.1:8080`), sem nada de container dentro do teste. Ver ADR-008.
module.exports = {
  testDir: '.',
  timeout: 120_000,
  expect: { timeout: 15_000 },
  // Um worker: os testes dividem a mesma fila de pareamento do servidor, e dois testes
  // procurando mesa ao mesmo tempo pareariam jogadores um do outro.
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL: process.env.BASE || 'http://127.0.0.1:8080',
    trace: 'retain-on-failure',
    video: 'retain-on-failure',
  },
};
