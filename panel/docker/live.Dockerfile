# Panel VizRS Live: statyczna binarka musl w obrazie scratch (jak VizRS CG System).
# Budowa (z katalogu repozytorium): docker build -f docker/live.Dockerfile -t vizrs-live .
# Wiele architektur (release.yml): docker buildx build --platform linux/amd64,linux/arm64 … – kompilacja krzyżowa
# na platformie budującej (rust-lld, serwer bez zależności), bez emulacji QEMU.
FROM --platform=$BUILDPLATFORM rust:1-alpine AS build
ARG TARGETARCH
ARG BUILDARCH
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock ./
COPY server/src ./src
COPY server/web ./web
RUN case "$TARGETARCH" in \
      amd64) T=x86_64; OPTS="" ;; \
      arm64) T=aarch64; OPTS="-C target-cpu=neoverse-n1 -C opt-level=3" ;; \
      *) echo "nieobsługiwana architektura: $TARGETARCH"; exit 1 ;; \
    esac \
 && T=$T-unknown-linux-musl && rustup target add $T \
 && RUSTFLAGS="$OPTS" CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
    cargo build --release --locked --target $T \
 && cp target/$T/release/vizrs-live /vizrs-live \
 && if [ "$TARGETARCH" = "$BUILDARCH" ]; then /vizrs-live --version; fi

FROM scratch
COPY --from=build /vizrs-live /vizrs-live
USER 10001:10001
ENV VIZRS_LIVE_BIND=0.0.0.0 \
    VIZRS_LIVE_PORT=8090
EXPOSE 8090
HEALTHCHECK --interval=15s --timeout=5s --start-period=5s --retries=3 CMD ["/vizrs-live", "--healthcheck"]
ENTRYPOINT ["/vizrs-live"]
