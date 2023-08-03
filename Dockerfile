FROM rust:alpine as rust 

WORKDIR /app
COPY . .

ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN apk add --update musl-dev 
RUN \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
  cargo install --path . --profile docker

FROM rustlang/rust:nightly-alpine

RUN apk add --update nodejs npm build-base && \
  cargo install rust-script && \
  cargo install wasm-pack

WORKDIR /app

RUN chmod 777 /app/
COPY --from=rust /app/template ./template
COPY --from=rust /usr/local/cargo/bin/rust-docker-isolation-provider .

RUN npm install @deep-foundation/deeplinks --prefix ./crates/

# `Rocket.toml` to change the port: https://rocket.rs/v0.5-rc/guide/configuration
ENV ROCKET_ADDRESS=0.0.0.0
EXPOSE 8000

CMD ["/app/rust-docker-isolation-provider"]

