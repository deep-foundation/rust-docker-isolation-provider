FROM rust:slim-buster as rust 

WORKDIR /app
COPY . .

RUN \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
  cargo install --path . --profile release-strip


FROM debian:stable-slim

COPY --from=rust /usr/local/cargo/bin/rust-docker-isolation-provider /usr/local/bin

# `Rocket.toml` to change the port: https://rocket.rs/v0.5-rc/guide/configuration
EXPOSE 8000

CMD ["rust-docker-isolation-provider"]

