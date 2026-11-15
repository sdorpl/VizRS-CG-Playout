# Składniki zewnętrzne

Kod VizRS Live: MIT ([LICENSE](LICENSE)). Panel steruje [ErsatzTV](https://ersatztv.org) legacy (Zlib) i korzysta z ErsatzTV Next (MIT) bez zmian w ich kodzie – powered by ErsatzTV.

| Składnik | Gdzie | Licencja |
|---|---|---|
| ErsatzTV legacy (oficjalny obraz `ghcr.io/ersatztv/legacy`, bez zmian) | obraz `ghcr.io/sdorpl/vizrs-live-legacy` = obraz upstream + `VizRS.Bridge.dll` i `commands.txt` w `/vizrs-bridge` (zmiana oznaczona etykietą OCI, `docker/legacy-bridge.Dockerfile`) | Zlib – https://github.com/ErsatzTV/legacy |
| ffmpeg i biblioteki z obrazu ErsatzTV | jak wyżej (warstwy obrazu upstream) | GPL / LGPL – źródła: https://ffmpeg.org |
| ASP.NET Core, MediatR (biblioteki, względem których kompiluje się most) | brane z obrazu legacy przy budowie, nie dołączane do `VizRS.Bridge.dll` | MIT / Apache-2.0 |
| Caddy (`caddy:alpine`, bez zmian) | usługa `proxy` w compose (obraz upstream) | Apache-2.0 |
| crate'y aplikacji `desktop/`: tauri, tauri-plugin-dialog, tauri-plugin-single-instance, serde_json, ureq (rustls), getrandom, webkit2gtk (+ zależności przechodnie) | AppImage i paczki okna | MIT / Apache-2.0 (rustls też ISC; wersje: `desktop/Cargo.lock`) |
