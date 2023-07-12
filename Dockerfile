FROM rust:slim-buster as rust 

WORKDIR /app
COPY . .

RUN \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
  cargo install --path . --profile docker

FROM rust:slim-buster 

RUN cargo install rust-script

WORKDIR /app

RUN chmod 777 /app/
COPY --from=rust /usr/local/cargo/bin/rust-docker-isolation-provider .

# `Rocket.toml` to change the port: https://rocket.rs/v0.5-rc/guide/configuration
ENV ROCKET_ADDRESS=0.0.0.0
EXPOSE 8000

CMD ["/app/rust-docker-isolation-provider"]

