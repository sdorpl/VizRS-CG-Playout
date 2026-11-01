# Seed of the stack (DOKUMENTACJA §4): the reference instance, baked in so the
# stack needs no files on the host. amd64 and arm64.
# Build from the repository root: docker build -f seed/Dockerfile .

FROM busybox:1.37
COPY seed/init.sh /seed/init.sh
COPY seed/vizrs-data /seed/vizrs-data
COPY seed/ersatztv-config /seed/ersatztv-config
COPY seed/media /seed/media
ENTRYPOINT ["sh", "/seed/init.sh"]
