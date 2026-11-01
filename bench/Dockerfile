# Sidecar benchmark (bench/sidecar.mjs): the Rust binary as in the sidecar
# image, headless Chromium and Node.js for the measuring script; amd64 and arm64.
# Build from the repository root: docker build -f bench/Dockerfile .

FROM --platform=$BUILDPLATFORM rust:1.97-bookworm AS build
ARG TARGETARCH
RUN case "$TARGETARCH" in \
      amd64) echo x86_64-unknown-linux-gnu > /target ;; \
      arm64) echo aarch64-unknown-linux-gnu > /target \
             && apt-get update \
             && apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
             && rm -rf /var/lib/apt/lists/* ;; \
      *) echo "unsupported architecture $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && rustup target add "$(cat /target)"
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --release --locked --target "$(cat /target)" \
    && cp "target/$(cat /target)/release/vizrs-cg" /vizrs-cg

FROM node:current-bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends chromium-headless-shell fonts-dejavu-core fonts-noto-core tzdata \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /vizrs-cg /usr/local/bin/vizrs-cg
WORKDIR /app
COPY tests/contract tests/contract
COPY bench bench

ENV HOME=/tmp \
    TZ=Europe/Warsaw \
    VIRTRS_CG_BIN=/usr/local/bin/vizrs-cg \
    VIZRS_BROWSER=/usr/lib/chromium/chromium-headless-shell \
    VIZRS_BROWSER_NO_SANDBOX=1
USER 10001
CMD ["node", "bench/sidecar.mjs"]
