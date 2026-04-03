FROM rust:1.86-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM gcr.io/distroless/static:nonroot

COPY --from=builder /app/target/release/polymarket-exporter /usr/local/bin/polymarket-exporter

EXPOSE 9184

ENV CONFIG_PATH="/etc/polymarket-exporter/config.toml"

ENTRYPOINT ["polymarket-exporter"]
