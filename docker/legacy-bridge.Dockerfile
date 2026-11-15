# ErsatzTV legacy z mostem VizRS (bez zmiany kodu legacy).
# Budowa (z katalogu repozytorium):
#   docker build -f docker/legacy-bridge.Dockerfile -t vizrs-live-legacy .
#   docker build -f docker/legacy-bridge.Dockerfile --build-arg LEGACY_IMAGE=ghcr.io/ersatztv/legacy:<wersja> -t vizrs-live-legacy .
# zawsze najnowsze oficjalne legacy (jak ersatztv i ingest w VizRS CG Playout); budowa z --pull
ARG LEGACY_IMAGE=ghcr.io/ersatztv/legacy:latest
ARG DOTNET_SDK=mcr.microsoft.com/dotnet/sdk:10.0

FROM ${LEGACY_IMAGE} AS legacy

# DLL mostu to kod IL niezależny od architektury: SDK zawsze na platformie budującej (buildx bez QEMU)
FROM --platform=$BUILDPLATFORM ${DOTNET_SDK} AS build
# MediatR prosto z obrazu legacy: wersja zawsze ta sama co w legacy
COPY --from=legacy /app/MediatR.dll /app/MediatR.Contracts.dll /legacy-lib/
COPY bridge/VizRS.Bridge.csproj bridge/Bridge.cs /src/
RUN dotnet build /src/VizRS.Bridge.csproj -c Release -o /out -p:LegacyDir=/legacy-lib --nologo -v q

FROM ${LEGACY_IMAGE}
# Zmieniona dystrybucja ErsatzTV legacy (licencja zlib: zmiana wyraźnie oznaczona): oficjalny obraz bez zmian
# + VizRS.Bridge.dll i commands.txt w /vizrs-bridge (MIT). ErsatzTV i ffmpeg w obrazie – THIRD-PARTY.md.
LABEL org.opencontainers.image.title="vizrs-live-legacy" \
      org.opencontainers.image.description="Oficjalny obraz ErsatzTV legacy (zlib; https://github.com/ErsatzTV/legacy) bez zmian + VizRS Bridge (MIT) w /vizrs-bridge. Zawiera ffmpeg z obrazu ErsatzTV (GPL/LGPL; źródła: https://ffmpeg.org). Powered by ErsatzTV." \
      org.opencontainers.image.licenses="MIT AND Zlib" \
      org.opencontainers.image.source="https://github.com/sdorpl/VizRS-Live-Playout"
COPY --from=build /out/VizRS.Bridge.dll /vizrs-bridge/VizRS.Bridge.dll
COPY bridge/commands.txt /vizrs-bridge/commands.txt
# DOTNET_gcServer=0: workstation GC – ok. połowa RAM legacy (DOKUMENTACJA §7). Bez limitu sterty
# (DOTNET_GCHeapHardLimit) na wszystkich platformach: po przekroczeniu legacy pada razem z emisją;
# strojenie pod Ampere A1 (limit, W^X) ustawia nakładka docker-compose.ampere.yml VizRS CG Playout.
ENV DOTNET_STARTUP_HOOKS=/vizrs-bridge/VizRS.Bridge.dll \
    ASPNETCORE_HOSTINGSTARTUPASSEMBLIES=VizRS.Bridge \
    DOTNET_gcServer=0
