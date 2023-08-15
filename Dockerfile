FROM rust:alpine as rust 

WORKDIR /app
COPY . .

ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN apk add --update musl-dev 
RUN \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
  RUSTFLAGS="--cfg docker_image" cargo install --path . --profile docker

FROM node:alpine as node
WORKDIR /app

RUN npm install @deep-foundation/deeplinks --prefix ./; \
    npm install @deep-foundation/hasura    --prefix ./

FROM rustlang/rust:nightly-alpine 
WORKDIR /app

RUN chmod 777 /app/
COPY --from=node /app/node_modules ./crates/node_modules 
COPY --from=rust /usr/local/cargo/bin/rust-docker-isolation-provider .
COPY --from=rust /app/template ./template

RUN apk add --update nodejs npm build-base 
RUN cargo install wasm-pack; rustup target add wasm32-unknown-unknown

ENV ROCKET_ADDRESS=0.0.0.0
ENV RUST_LOG="rust_docker_isolation_provider=info"

CMD ["/app/rust-docker-isolation-provider"]

