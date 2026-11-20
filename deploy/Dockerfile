FROM rust:1.99.1-bookworm AS build
ARG BIN
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked --bin "$BIN" && cp "target/release/$BIN" /runtime

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /runtime /usr/local/bin/runtime
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/runtime"]
