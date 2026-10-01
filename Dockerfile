# Duas etapas: compila numa imagem com cargo, roda numa imagem sem.
FROM rust:1-slim-trixie AS construcao
WORKDIR /obra

# As dependências primeiro, com fontes vazias: assim `docker build` só refaz o download
# quando o Cargo.toml muda, e não a cada edição de código.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/regras/Cargo.toml crates/regras/
COPY crates/servidor/Cargo.toml crates/servidor/
RUN mkdir -p crates/regras/src crates/servidor/src cliente \
 && echo 'pub fn nada() {}' > crates/regras/src/lib.rs \
 && echo 'pub fn nada() {}' > crates/servidor/src/lib.rs \
 && echo 'fn main() {}' > crates/servidor/src/main.rs \
 && echo '<!doctype html>' > cliente/index.html \
 && cargo build --release --locked \
 && rm -rf crates/regras/src crates/servidor/src cliente

COPY crates crates
COPY cliente cliente
# `touch` porque o cargo decide por data de modificação, e o COPY pode preservar uma
# anterior à do build de dependências — nesse caso ele não recompilaria o nosso código.
RUN touch crates/regras/src/lib.rs crates/servidor/src/lib.rs crates/servidor/src/main.rs \
 && cargo build --release --locked --bin truco

FROM debian:trixie-slim
# ca-certificates: os webhooks saem por HTTPS para destino de terceiro.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
# Usuário sem privilégio. O processo não precisa de root para nada, e o container também não.
RUN useradd --system --create-home --uid 10001 truco
COPY --from=construcao /obra/target/release/truco /usr/local/bin/truco
# O banco vive num volume: `docker run --rm` não pode apagar o saldo de ninguém.
RUN mkdir -p /dados && chown truco:truco /dados
USER truco
VOLUME ["/dados"]
ENV TRUCO_ENDERECO=0.0.0.0:8080 \
    TRUCO_BANCO=sqlite:///dados/truco.db
EXPOSE 8080
# O próprio binário pergunta a si mesmo (`--saude`): assim a imagem final não precisa de
# curl, e ferramenta de rede em imagem de produção é superfície que não compra nada.
HEALTHCHECK --interval=10s --timeout=3s --start-period=3s --retries=3 \
  CMD ["/usr/local/bin/truco", "--saude"]
ENTRYPOINT ["/usr/local/bin/truco"]
