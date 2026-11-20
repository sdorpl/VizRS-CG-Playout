# Składniki zewnętrzne

Kod VizRS CG Playout (sidecar i panel `panel/`, dawny VizRS Live): MIT ([LICENSE](LICENSE)). Współpracuje z [ErsatzTV](https://ersatztv.org) (legacy: Zlib, Next: MIT) bez zmian w jego kodzie i bez kopiowania go – powered by ErsatzTV.

| Składnik | Gdzie | Licencja |
|---|---|---|
| crate'y `png`, `regex` (+ zależności przechodnie) | binarki `vizrs-cg`, `vizrs-ffmpeg`, `vizrs-etv-client` | MIT / Apache-2.0 (wersje: `Cargo.lock`) |
| `vizrs-render` (renderer natywny) | obraz `vizrs-cg`, budowany z VizRS CG System (`VIZRS_REF` w `Dockerfile`) | MIT; fonty w nim – SIL OFL 1.1 (VizRS CG System `THIRD-PARTY.md`) |
| Chromium headless shell, fonty DejaVu i Noto, tzdata, biblioteki Debiana | obraz `ghcr.io/sdorpl/vizrs-cg` (pakiety Debian bookworm) | BSD-3-Clause i inne licencje Chromium; fonty: licencja Bitstream Vera/DejaVu, SIL OFL 1.1; pozostałe – wg pakietów Debiana (`/usr/share/doc/*/copyright` w obrazie) |
| ffmpeg | obraz ErsatzTV (nie rozpowszechniamy go w obrazie `vizrs-cg`); kontener `ingest` używa obrazu ErsatzTV legacy | GPL / LGPL – źródła: https://ffmpeg.org, obraz: https://github.com/ErsatzTV/legacy |
| yt-dlp (samodzielna binarka `yt-dlp_linux` / `yt-dlp_linux_aarch64`, wersja w `Dockerfile`) | obraz `vizrs-cg` (`/usr/local/share/vizrs/client/yt-dlp`), instalowany na wolumen `vizrs-client` dla usługi `ingest` | Unlicense; Python i biblioteki w środku – PSF i licencje wg `THIRD_PARTY_LICENSES.txt` wydania yt-dlp (https://github.com/yt-dlp/yt-dlp) |
| Deno (oficjalna binarka `deno-{x86_64,aarch64}-unknown-linux-gnu`, wersja i sumy w `Dockerfile`) | obraz `vizrs-cg` (`/usr/local/share/vizrs/client/deno`), instalowany na wolumen `vizrs-client` – środowisko JavaScript yt-dlp w usłudze `ingest` | MIT; biblioteki w środku (V8 – BSD-3-Clause, Rust crates) wg https://github.com/denoland/deno |
| dane startowe `seed/media` | obraz `vizrs-seed` | wg plików w `seed/` |
| ErsatzTV legacy (oficjalny obraz `ghcr.io/ersatztv/legacy`, bez zmian) | obraz `ghcr.io/sdorpl/vizrs-live-legacy` = obraz upstream + `VizRS.Bridge.dll` i `commands.txt` w `/vizrs-bridge` (zmiana oznaczona etykietą OCI, `panel/docker/legacy-bridge.Dockerfile`) | Zlib – https://github.com/ErsatzTV/legacy |
| ASP.NET Core, MediatR (biblioteki, względem których kompiluje się most) | brane z obrazu legacy przy budowie, nie dołączane do `VizRS.Bridge.dll` | MIT / Apache-2.0 |
| Caddy (`caddy:alpine`, bez zmian) | usługa `proxy` w nakładce panelu (obraz upstream) | Apache-2.0 |
| crate'y aplikacji okna panelu (`panel/desktop/`): tauri, tauri-plugin-dialog, tauri-plugin-single-instance, serde_json, ureq (rustls), getrandom, webkit2gtk (+ zależności przechodnie) | AppImage i paczki okna | MIT / Apache-2.0 (rustls też ISC; wersje: `panel/desktop/Cargo.lock`) |
