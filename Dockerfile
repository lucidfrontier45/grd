ARG TARGET=x86_64-unknown-linux-musl

FROM rust:1.93-slim AS builder
ARG TARGET
WORKDIR /workspace

RUN apt-get update && apt-get install -y musl-tools && \
    rustup target add ${TARGET}

COPY . .
RUN cargo build --release --target ${TARGET}

FROM alpine:latest
ARG TARGET
WORKDIR /bin
COPY --from=builder /workspace/target/${TARGET}/release/grd .

ENTRYPOINT ["./grd"]