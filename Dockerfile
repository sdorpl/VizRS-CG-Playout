# VizRS CG Playout sidecar: Rust binary + vizrs-render + headless Chromium (CPU; GPU with
# VIZRS_BROWSER_GPU=1 and docker-compose.nvidia.yml – libegl1 loads the NVIDIA EGL driver),
# amd64 and arm64 (tuned for Ampere A1 Neoverse-N1 on arm64). vizrs-render comes
# from the VizRS CG System release VIZRS_REF.
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
      amd64) echo x86_64-unknown-linux-gnu > /target; echo "" > /rust-opts ;; \
      arm64) echo aarch64-unknown-linux-gnu > /target; echo "-C target-cpu=neoverse-n1 -C opt-level=3" > /rust-opts \
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
# yt-dlp for YouTube inputs of live ingest (src/listener.rs): the standalone
# build (Python inside, no Python in the ErsatzTV image needed), installed by
# the sidecar next to vizrs-ffmpeg; pinned version and checksums.
ARG YTDLP_VERSION=2026.10.26
ARG YTDLP_SHA256_AMD64=767ba9dce7052aa196f8b12652f433d76de128ba168966fa84b6394d5e6c6795
ARG YTDLP_SHA256_ARM64=e87e9533e89fe5889abaa21f5114dfe7756d6aa6219b83551add04d48b18e483
RUN case "$TARGETARCH" in \
      amd64) file=yt-dlp_linux; sum="$YTDLP_SHA256_AMD64" ;; \
      arm64) file=yt-dlp_linux_aarch64; sum="$YTDLP_SHA256_ARM64" ;; \
    esac \
    && mkdir -p /client \
    && curl -fsSL --retry 3 -o /client/yt-dlp "https://github.com/yt-dlp/yt-dlp/releases/download/$YTDLP_VERSION/$file" \
    && echo "$sum  /client/yt-dlp" | sha256sum -c - \
    && chmod 755 /client/yt-dlp
# Deno: the JavaScript runtime yt-dlp solves YouTube's challenges with. With
# an account's cookies (a server YouTube asks to sign in) yt-dlp takes the
# web clients, whose stream addresses need it ("n challenge"); pinned
# version and checksums of the release zips.
ARG DENO_VERSION=v2.9.8
ARG DENO_SHA256_AMD64=e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609
ARG DENO_SHA256_ARM64=d49596c20b9788cf8f5d8ddf644103d4bde56ed239753f0bdb92ba9361d51905
RUN case "$TARGETARCH" in \
      amd64) file=deno-x86_64-unknown-linux-gnu.zip; sum="$DENO_SHA256_AMD64" ;; \
      arm64) file=deno-aarch64-unknown-linux-gnu.zip; sum="$DENO_SHA256_ARM64" ;; \
    esac \
    && curl -fsSL --retry 3 -o /tmp/deno.zip "https://github.com/denoland/deno/releases/download/$DENO_VERSION/$file" \
    && echo "$sum  /tmp/deno.zip" | sha256sum -c - \
    && python3 -c "import zipfile; zipfile.ZipFile('/tmp/deno.zip').extract('deno', '/client')" \
    && chmod 755 /client/deno \
    && rm /tmp/deno.zip
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN RUSTFLAGS="$(cat /rust-opts)" cargo build --release --locked --target "$(cat /target)" \
    && cp "target/$(cat /target)/release/vizrs-cg" /vizrs-cg \
    && RUSTFLAGS="$(cat /rust-opts)" cargo build --release --locked --bin vizrs-etv-client --bin vizrs-ffmpeg --target "$(cat /target-musl)" \
    && mkdir -p /client \
    && cp "target/$(cat /target-musl)/release/vizrs-etv-client" "target/$(cat /target-musl)/release/vizrs-ffmpeg" /client/

ARG VIZRS_REF=v26.11.13
RUN git clone --depth 1 --branch "$VIZRS_REF" https://github.com/sdorpl/VizRS-CG-System.git /vizrs \
    && cd /vizrs/source/render \
    && RUSTFLAGS="$(cat /rust-opts)" cargo build --release --locked --target "$(cat /target)" \
    && cp "target/$(cat /target)/release/vizrs-render" /vizrs-render

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends chromium-headless-shell libegl1 fonts-dejavu-core fonts-noto-core tzdata \
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

USER 10001:10001
EXPOSE 7700
CMD ["vizrs-cg"]
