FROM rust:1.99.1-bookworm AS build
ARG BIN
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked --bin "$BIN" && cp "target/release/$BIN" /runtime

FROM debian:bookworm-slim AS signing-helper
ARG AWS_SIGNING_HELPER_VERSION=1.8.5
ARG AWS_SIGNING_HELPER_SHA256=beec9ed1c492d93db809890f16713e3556353294b823c2184ad4e891f1b2b54d
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/* \
    && curl --fail --location --silent --show-error \
      "https://rolesanywhere.amazonaws.com/releases/${AWS_SIGNING_HELPER_VERSION}/X86_64/Linux/Amzn2023/aws_signing_helper" \
      --output /aws_signing_helper \
    && echo "${AWS_SIGNING_HELPER_SHA256}  /aws_signing_helper" | sha256sum --check --strict \
    && chmod 0555 /aws_signing_helper

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /runtime /usr/local/bin/runtime
COPY --from=signing-helper /aws_signing_helper /usr/local/bin/aws_signing_helper
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/runtime"]
