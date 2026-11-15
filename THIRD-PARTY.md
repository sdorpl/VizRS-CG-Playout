# Składniki zewnętrzne

Kod VizRS CG Playout: MIT ([LICENSE](LICENSE)). Współpracuje z [ErsatzTV](https://ersatztv.org) (legacy: Zlib, Next: MIT) bez zmian w jego kodzie i bez kopiowania go – powered by ErsatzTV.

| Składnik | Gdzie | Licencja |
|---|---|---|
| crate'y `png`, `regex` (+ zależności przechodnie) | binarki `vizrs-cg`, `vizrs-ffmpeg`, `vizrs-etv-client` | MIT / Apache-2.0 (wersje: `Cargo.lock`) |
| `vizrs-render` (renderer natywny) | obraz `vizrs-cg`, budowany z VizRS CG System (`VIZRS_REF` w `Dockerfile`) | MIT; fonty w nim – SIL OFL 1.1 (VizRS CG System `THIRD-PARTY.md`) |
| Chromium headless shell, fonty DejaVu i Noto, tzdata, biblioteki Debiana | obraz `ghcr.io/sdorpl/vizrs-cg` (pakiety Debian bookworm) | BSD-3-Clause i inne licencje Chromium; fonty: licencja Bitstream Vera/DejaVu, SIL OFL 1.1; pozostałe – wg pakietów Debiana (`/usr/share/doc/*/copyright` w obrazie) |
| ffmpeg | obraz ErsatzTV (nie rozpowszechniamy go w obrazie `vizrs-cg`); kontener `ingest` używa obrazu ErsatzTV legacy | GPL / LGPL – źródła: https://ffmpeg.org, obraz: https://github.com/ErsatzTV/legacy |
| dane startowe `seed/media` | obraz `vizrs-seed` | wg plików w `seed/` |
