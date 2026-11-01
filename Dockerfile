# VizRS CG Playout sidecar: Rust binary + vizrs-render + headless Chromium (no GPU),
# amd64 and arm64. vizrs-render comes from the VizRS CG System release VIZRS_REF.
# The binary is built on the build host's platform and cross-compiled for the
# target (pure Rust, no C libraries): no emulation, minutes instead of an hour
# under QEMU. chromium-headless-shell renders /output with about half the CPU
# of the full browser (no browser UI layer); same engine as `chromium`.

FROM --platform=$BUILDPLATFORM rust:1.97-bookworm AS build
ARG TARGETARCH
ARG BUILDARCH
# Built on arm64 itself (e.g. `up --build` on an Ampere A1) the native gcc
# links: Debian has no cross gcc for aarch64 on arm64 hosts.
RUN case "$TARGETARCH" in \
      amd64) echo x86_64-unknown-linux-gnu > /target ;; \
      arm64) echo aarch64-unknown-linux-gnu > /target \
             && if [ "$BUILDARCH" != arm64 ]; then \
                  dpkg --add-architecture arm64 \
                  && apt-get update \
                  && apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
                  && rm -rf /var/lib/apt/lists/*; \
                fi ;; \
      *) echo "unsupported architecture $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && rustup target add "$(cat /target)" \
    && sed 's/-gnu$/-musl/' /target > /target-musl \
    && rustup target add "$(cat /target-musl)"
# The native gcc of Debian arm64 has the same name.
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --release --locked --target "$(cat /target)" \
    && cp "target/$(cat /target)/release/vizrs-cg" /vizrs-cg \
    && cargo build --release --locked --bin vizrs-etv-client --bin vizrs-ffmpeg --target "$(cat /target-musl)" \
    && mkdir /client \
    && cp "target/$(cat /target-musl)/release/vizrs-etv-client" "target/$(cat /target-musl)/release/vizrs-ffmpeg" /client/

ARG VIZRS_REF=v2.0.12
RUN git clone --depth 1 --branch "$VIZRS_REF" https://github.com/sdorpl/VizRS-CG-System.git /vizrs \
    && cd /vizrs/source/render \
    && cargo build --release --locked --target "$(cat /target)" \
    && cp "target/$(cat /target)/release/vizrs-render" /vizrs-render

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends chromium-headless-shell fonts-dejavu-core fonts-noto-core tzdata \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /vizrs-cg /usr/local/bin/vizrs-cg
COPY --from=build /vizrs-render /usr/local/bin/vizrs-render
# Static programs for the ErsatzTV container; the sidecar installs them (DOKUMENTACJA §4).
COPY --from=build /client /usr/local/share/vizrs/client
COPY etv/templates/graphics-elements/script/vizrs.yml /usr/local/share/vizrs/vizrs.yml
WORKDIR /app
COPY etv etv

ENV VIZRS_RENDERER_BIN=/usr/local/bin/vizrs-render \
    VIZRS_CLIENT_SOURCE=/usr/local/share/vizrs/client
# Chromium needs a writable HOME (crashpad) for the unprivileged user. The
# browser binary itself, not Debian's /usr/bin wrapper: the wrapper runs it as
# a child of sh, so stopping and the parent-death signal would only reach sh.
ENV HOME=/tmp \
    TZ=Europe/Warsaw \
    VIZRS_BROWSER=/usr/lib/chromium/chromium-headless-shell \
    VIZRS_BROWSER_NO_SANDBOX=1 \
    LISTEN=0.0.0.0:7700

USER 10001
EXPOSE 7700
CMD ["vizrs-cg"]
