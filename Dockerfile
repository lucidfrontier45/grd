FROM rust:1.93-slim AS builder
WORKDIR /workspace

RUN apt-get update && apt-get install -y musl-tools && \
    rustup target add x86_64-unknown-linux-musl

COPY . .

RUN cargo build --release --target x86_64-unknown-linux-musl

FROM alpine:latest

WORKDIR /app
COPY --from=builder /workspace/target/x86_64-unknown-linux-musl/release/grd .

ENTRYPOINT ["./grd"]