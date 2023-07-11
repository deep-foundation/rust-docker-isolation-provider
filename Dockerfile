FROM rust:slim-buster

COPY . /app
WORKDIR /app

RUN \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/home/root/app/target \
  cargo +stable build --profile release-strip

# `Rocket.toml` to change the port: https://rocket.rs/v0.5-rc/guide/configuration
EXPOSE 8000

CMD ["target/release/rust-docker-isolation-provider"]
