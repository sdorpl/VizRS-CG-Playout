# VizRS Live – runbook administratora

Wdrożenie, kopie, aktualizacja legacy, odtworzenie. Praca operatora: [INSTRUKCJA.md](INSTRUKCJA.md); budowa i API: [DOKUMENTACJA.md](DOKUMENTACJA.md).

## 1. Składniki i porty

| Usługa (`compose.yml`) | Obraz (wydanie 1.2.3) | Port na zewnątrz |
|---|---|---|
| `live` – panel | `ghcr.io/sdorpl/vizrs-live:1.2.3` | 8090 – operatorzy |
| `ersatztv` – legacy z mostem | `vizrs-live-legacy:local` – budowany na miejscu na najnowszym `ghcr.io/ersatztv/legacy` + VizRS Bridge, ffmpeg z VAAPI/QSV/NVENC (§2b) | brak (`expose`) |
| `proxy` – Caddy | `caddy:alpine` | 8409 – widzowie: strumienie, M3U, XMLTV, HDHomeRun |

Dane: `ETV_CONFIG` (domyślnie `./etv-config`, cała baza i stan legacy) i `MEDIA_DIR` (media, tylko do odczytu). Panel nie trzyma żadnych danych.

## 2. Wdrożenie

1. Docker z Compose v2. Skopiuj repozytorium (albo tylko `compose.yml`, `.env.example`, `docker/proxy.Caddyfile`).
2. `cp .env.example .env`; wygeneruj dwa **różne** tokeny: `head -c 24 /dev/urandom | base64 | tr -d '/+='` → `VIZRS_BRIDGE_TOKEN`, `VIZRS_LIVE_TOKEN`. Ustaw `ETV_CONFIG`, `MEDIA_DIR`, `TZ`, `VIZRS_LIVE_STREAM_URL=http://<serwer>:8409` (adres dla widzów, widoczny z kontenera panelu). `chmod 600 .env`.
3. `docker compose pull live && docker compose up -d` – panel z ghcr, legacy z mostem budowany na miejscu (`pull_policy: build`); każde `up` sprawdza nowsze `ghcr.io/ersatztv/legacy` i dobudowuje most (warstwy z pamięci podręcznej, bez nowego legacy – kilka sekund).
4. Sprawdź: `docker compose ps` (wszystko `healthy`/`running`), panel `http://<serwer>:8090/` → zaloguj → **Diagnostyka**: most zielony, „Web GUI legacy dla widzów” = zamknięte. Z innej maszyny: `curl -I http://<serwer>:8409/` → 403, `curl -I http://<serwer>:8409/iptv/channels.m3u` → 200.
5. Pierwsza konfiguracja (biblioteka, kanał, harmonogram, playout) – w panelu; legacy nie wymaga kreatora.

**Sieć i TLS.** Panel i proxy mówią zwykłym HTTP. Panel wystawiaj tylko w sieci zaufanej (LAN/VPN); przez Internet – wyłącznie za odwrotnym proxy z TLS (np. Caddy z certyfikatem), które ustawia `X-Forwarded-Proto: https` – panel doda wtedy `Secure` do ciasteczka. Port 8090 nie powinien być widoczny publicznie bez TLS (token leci w treści logowania i w ciasteczku).

**CG Playout (opcjonalnie).** `VIZRS_LIVE_CG_URL=http://cg:7700` (sieć wewnętrzna) i `VIZRS_LIVE_CG_TOKEN` = `panel_token` CG Playout.

**VizRS CG System (opcjonalnie, podgląd grafiki i Trio).** `VIZRS_LIVE_VIZRS_URL` – adres VizRS widziany z przeglądarki operatora (nie z kontenera): `http://<serwer>:8788` albo sam port `:8788` (ten sam host co panel – zalecane, ciasteczko Trio działa wtedy także w ramce). VizRS z tokenem: `VIZRS_LIVE_VIZRS_OUTPUT_TOKEN` = `VIZRS_OUTPUT_TOKEN` VizRS (tylko odczyt). Anteny kanałów (`VIZRS_CHANNEL_BUSES=1` w CG Playout): `VIZRS_LIVE_VIZRS_CHANNELS=1`. Sprawdzenie: Grafika kanału → ramka pokazuje wyjście (nie „wymagany token”), Diagnostyka → Panel.

### 2a. Razem ze stosem VizRS CG Playout (jeden projekt compose)

Gdy ErsatzTV legacy działa już w stosie CG Playout (`../VirtRS CG Playout/docker-compose.yml`: `vizrs`, `cg`, `ersatztv`, `ingest`), VizRS Live wpina się w ten sam projekt nakładką `compose.cg-playout.yml` zamiast osobnego `compose.yml`: `ersatztv` dostaje obraz legacy z mostem i traci port na zewnątrz, dochodzą `proxy` (port 8409 dla widzów) i `live` (8090, CG Playout przez `http://cg:7700`). Wolumen `ersatztv-config` i reszta stosu bez zmian.

1. Panel z repozytorium (albo gotowy z ghcr): `VIZRS_BRIDGE_TOKEN=x… VIZRS_LIVE_TOKEN=x… LIVE_IMAGE=vizrs-live:local docker compose build live` w VizRS Live; w CG Playout `docker compose build cg`. Legacy z mostem nakładka buduje sama przy każdym `up` na najnowszym `ghcr.io/ersatztv/legacy` (tym samym, co `ingest` CG Playout).
2. Kopia `ersatztv-config` (§3; wolumen: `docker run --rm -v <projekt>_ersatztv-config:/c -v "$PWD":/k alpine tar -C /c -czf /k/etv-config-$(date +%F).tgz .` przy zatrzymanym `ersatztv`) – nowsze legacy migruje bazę starszej wersji.
3. W `.env` CG Playout (poza gitem): `COMPOSE_FILE=docker-compose.yml:docker-compose.nvidia.yml:<ścieżka VizRS Live>/compose.cg-playout.yml` (bez nakładki NVIDIA – bez niej), `VIZRS_LIVE_DIR=<ścieżka VizRS Live>` (dla `docker/proxy.Caddyfile`), `VIZRS_BRIDGE_TOKEN`, `VIZRS_LIVE_TOKEN` (dwa różne, §2), `LIVE_IMAGE` (`LIVE_LEGACY_IMAGE` – opcjonalnie, nazwa lokalnego obrazu z mostem, domyślnie `vizrs-live-legacy:local`), `VIZRS_LIVE_STREAM_URL=http://<serwer>:8409`; token CG dla panelu to `VIZRS_PANEL_TOKEN`, który już tam jest. VizRS dla panelu nakładka ustawia sama: adres `:${VIZRS_PORT}` (host, pod którym otwarto panel), token wyjścia `VIZRS_OUTPUT_TOKEN`, anteny kanałów wg `VIZRS_CHANNEL_BUSES`; własny `VIZRS_LIVE_VIZRS_URL` w `.env` ma pierwszeństwo – działa tylko, gdy operator otwiera panel pod tym samym hostem (np. `127.0.0.1` ≠ `localhost`).
4. `docker compose up -d` w katalogu CG Playout; sprawdzenie jak w §2 pkt 4 (most zielony, `curl -I :8409/` → 403, `:8409/iptv/channels.m3u` → 200) i w panelu Sloty na żywo (CG Playout połączony).

Cofnięcie: przywróć poprzedni `.env` CG Playout i kopię `ersatztv-config` (§5), `docker compose up -d --remove-orphans`.

**Portainer (stos z GitHuba, np. s1):** `compose.cg-playout.yml` montuje pliki z `VIZRS_LIVE_DIR` i buduje legacy na miejscu, czego stos z repozytorium w Portainerze nie potrafi. Tam zamiast niej służy `docker-compose.live.yml` z repozytorium CG Playout (obrazy z ghcr – `vizrs-live` i `vizrs-live-legacy` w wersji wydania, Caddyfile wpisany w compose): w stosie **Additional paths** → `docker-compose.live.yml` (po `docker-compose.ampere.yml`), w **Environment variables** `VIZRS_BRIDGE_TOKEN`, `VIZRS_LIVE_TOKEN`, `VIZRS_PANEL_TOKEN`, `VIZRS_LIVE_STREAM_URL`; potem **Pull and redeploy**. Przed pierwszym razem kopia `ersatztv-config` (pkt 2) i porównanie wersji legacy w obrazie z mostem z działającą – starszy obraz nie może dostać nowszej bazy. Opis nakładki: DOKUMENTACJA CG Playout, „VizRS Live zamiast web GUI ErsatzTV”.

**Media w panelu.** Usługa `live` montuje media tak jak legacy (`${MEDIA_DIR:-seed-media}:${MEDIA_MOUNT:-/media}:ro` w obu nakładkach – `MEDIA_MOUNT` jak w stosie CG Playout, np. `/media/tv` na s2, `${MEDIA_DIR:-./media}:/media:ro` w `compose.yml`) – Przeglądarka gra pliki lokalne bez legacy (DOKUMENTACJA §3 `/api/vod`). Inna ścieżka mediów w legacy = ta sama w panelu, inaczej pliki lokalne grają tylko przez legacy. Pozycje Jellyfin nie potrzebują montowania (plik z Jellyfin przez serwer panelu).

**Dwie nakładki, jedna treść.** `compose.cg-playout.yml` (stos lokalny) i `docker-compose.live.yml` CG Playout (Portainer) opisują to samo. Caddyfile, środowisko i wolumeny usługi `live` oraz nakładka VAAPI muszą być w nich takie same – sprawdza to `scripts/check-cg-overlay.sh [katalog CG Playout]`, a `tests/deploy.sh` uruchamia go na początku i przerywa wdrożenie przy rozjeździe. Zmieniasz jedno – popraw drugie w tym samym cyklu (źródłem jest VizRS Live: `docker/proxy.Caddyfile`, `compose.cg-playout.yml`).

### 2b. Akceleracja sprzętowa (Intel, AMD, NVIDIA)

Kodowanie wideo wykonuje ffmpeg w procesie ErsatzTV legacy (silnik Next); obraz panelu `vizrs-live` (`scratch`) nie zawiera ffmpeg – akceleracja dotyczy wyłącznie usługi `ersatztv` (obraz `vizrs-live-legacy`, lokalnie `vizrs-live-legacy:local`).

Oficjalny obraz bazowy `ghcr.io/ersatztv/legacy` (Ubuntu 24.04, ffmpeg 8.1.2-etv, amd64) zawiera już komplet koderów i sterowników:
- **`ffmpeg -hwaccels`**: `vaapi`, `qsv`, `cuda`;
- **kodery**: `h264_vaapi`, `hevc_vaapi`, `av1_vaapi`, `h264_qsv`, `hevc_qsv`, `av1_qsv`, `h264_nvenc`, `hevc_nvenc`;
- **sterowniki**: Intel VAAPI `iHD` (od generacji Broadwell) i `i965` (starszy Intel), AMD VAAPI `radeonsi` (Mesa); Intel QSV (`libmfx-gen` / Media SDK); `libva`.
- **NVIDIA**: biblioteki sterownika (`libcuda`, `libnvidia-encode`) muszą odpowiadać wersji sterownika na hoście, dlatego nie mogą znajdować się wewnątrz obrazu – dostarcza je w locie NVIDIA Container Toolkit przez CDI.
- **AMD**: AMD AMF (sterownik zamknięty) nie jest używany; karty AMD działają przez otwarte sterowniki VAAPI (Mesa `radeonsi`).

Most VizRS Bridge nie ingeruje w ffmpeg ani system bazowy (obraz legacy bez zmian).

| Karta | Nakładka | Na hoście | W panelu (Profile kodowania → Akceleracja) |
|---|---|---|---|
| **Intel** (iGPU, Arc) | `compose.vaapi.yml` | sterownik jądra `i915` albo `xe` (`/dev/dri/renderD128`) | **VAAPI** (urządzenie `/dev/dri/renderD128`, sterownik `iHD` lub `i965`) albo **QSV** |
| **AMD** (Radeon) | `compose.vaapi.yml` | sterownik jądra `amdgpu` (`/dev/dri/renderD128`) | **VAAPI** (urządzenie `/dev/dri/renderD128`, sterownik `RadeonSI`) |
| **NVIDIA** | `compose.nvidia.yml` | sterownik NVIDIA + `nvidia-container-toolkit` + CDI (`/var/run/cdi/nvidia.yaml`) | **NVIDIA** |

#### Wymagania na hoście i nakładki compose

1. **Intel i AMD – VAAPI / QSV (`compose.vaapi.yml`)**:
   - Nakładka przekazuje urządzenie `${VAAPI_DEVICE:-/dev/dri/renderD128}:/dev/dri/renderD128` do kontenera `ersatztv`.
   - W przypadku kilku kart graficznych wskaż właściwy węzeł za pomocą zmiennej środowiskowej `VAAPI_DEVICE` (np. sprawdzając `ls -l /dev/dri/by-path`).
   - Proces legacy w kontenerze działa jako `root`, więc dodawanie użytkownika do grupy `render` na hoście nie jest wymagane.

2. **NVIDIA – NVENC / CUDA (`compose.nvidia.yml`)**:
   - Ustawia `NVIDIA_DRIVER_CAPABILITIES=compute,video,utility` i przekazuje kartę przez mechanizm CDI (`driver: cdi`, `device_ids: ["nvidia.com/gpu=${NVIDIA_GPU:-all}"]`) – nie wymaga przestarzałego runtime `nvidia` w Dockerze.
   - Na hoście wymagany jest sterownik NVIDIA oraz pakiet `nvidia-container-toolkit`.
   - Wygeneruj specyfikację CDI: `sudo nvidia-ctk cdi generate --output=/var/run/cdi/nvidia.yaml`.
   - Sprawdź dostępność urządzeń: `nvidia-ctk cdi list`.
   - Przy kilku kartach NVIDIA można wybrać konkretną przez zmienną `NVIDIA_GPU` (numer np. `0` lub UUID `GPU-…`, domyślnie `all`).

#### Uruchomienie

- **Samodzielny stos VizRS Live**:
  ```bash
  docker compose -f compose.yml -f compose.vaapi.yml up -d     # Intel / AMD
  docker compose -f compose.yml -f compose.nvidia.yml up -d    # NVIDIA
  ```
- **W stosie VizRS CG Playout**: dołóż nakładkę po `compose.cg-playout.yml` (obie nakładki celują w usługę `ersatztv`):
  ```bash
  docker compose -f docker-compose.yml -f "$VIZRS_LIVE_DIR/compose.cg-playout.yml" -f "$VIZRS_LIVE_DIR/compose.vaapi.yml" up -d
  # lub w pliku .env CG Playout w COMPOSE_FILE dopisz nakładkę po compose.cg-playout.yml
  ```

#### Ustawienie w panelu

W menu przejdź do **Profile kodowania** → edytuj wybrany profil (lub utwórz nowy):
- W sekcji **Akceleracja sprzętowa** wybierz:
  - dla Intel/AMD: **VAAPI** (urządzenie: `/dev/dri/renderD128`, sterownik: `iHD` dla nowszego Intela, `i965` dla starszego, `RadeonSI` dla AMD) lub **QSV** (Intel);
  - dla NVIDIA: **NVIDIA**.
- Zapisz profil i upewnij się, że jest przypisany do kanału w menu **Kanały**.

#### Sprawdzenie poprawności

1. **Test obrazu bez GPU**: uruchom skrypt `tests/hwaccel.sh [obraz]` (domyślnie `vizrs-live-legacy:dev`). Skrypt weryfikuje obecność akceleracji, koderów i sterowników wewnątrz kontenera i zwraca kod ≠ 0 przy jakimkolwiek braku (używany w CI i przy wydaniach).
2. **W panelu**:
   - **Diagnostyka** → panel „Stan” / „Kontrole legacy” / „Hardware Acceleration”: weryfikuje zgłaszane akceleratory; panel pokazuje także wykryte karty grafiki oraz wersje i możliwości ffmpeg.
   - **Logi**: podczas transkodowania i nadawania sprawdź logi ffmpeg legacy (oraz raporty w `etv-config/ffmpeg-reports`), aby upewnić się, że strumień kodowany jest przez sprzętowy enkoder (np. `h264_vaapi`, `h264_nvenc`, `h264_qsv`).

## 3. Kopie zapasowe `etv-config`

Baza to SQLite w trybie WAL (`ersatztv.sqlite3`, `-wal`, `-shm`) – kopia na działającym legacy może być niespójna.

```bash
docker compose stop ersatztv
tar -C "$(dirname "$ETV_CONFIG")" -czf etv-config-$(date +%F).tgz \
    --exclude='*/cache/transcode' --exclude='*/temp-pool' "$(basename "$ETV_CONFIG")"
docker compose start ersatztv
```

- Przerwa w nadawaniu: kilkanaście sekund – rób w nocy, cron raz na dobę, trzymaj min. 7 kopii, jedną poza serwerem.
- Zawsze **przed** aktualizacją legacy (nowsze legacy migruje bazę, starsze już jej nie otworzy).
- W kopii są sekrety (`*-secrets.json`: klucze Jellyfin/Emby/Plex, `api-secrets.json`) – szyfruj kopię poza serwerem.
- `.env` (tokeny) kopiuj osobno.
- W `etv-config` jest też katalog mostu `vizrs-bridge/` (klatkaż profili kodowania, `framerates.json`) – kopia całego wolumenu go obejmuje.

## 4. Aktualizacja

**Panel** (nowe wydanie VizRS Live): domyślnie `LIVE_IMAGE` to `:latest`, więc wystarczy `docker compose pull live && docker compose up -d` (przypięcie wersji: `LIVE_IMAGE=ghcr.io/sdorpl/vizrs-live:<wersja>` w `.env`) (most dobuduje się z repozytorium – najpierw `git pull`).

**Legacy** – zawsze najnowsze `ghcr.io/ersatztv/legacy` (decyzja 2026-11-07, jak w CG Playout): każde `docker compose up -d` pobiera nowsze legacy, jeśli jest, i buduje na nim most. Nowsze legacy migruje bazę bez pytania, więc:
1. Przed `up`, które może przynieść nowe legacy, zrób kopię `etv-config` (§3). Wcześniejsze sprawdzenie: `tests/stack.sh up && tests/stack.sh seed && tests/stack.sh test` na maszynie testowej (DOKUMENTACJA §6) – stos testowy też bierze najnowsze.
2. `docker compose up -d`.
3. Diagnostyka: most zielony, brak brakujących komend; Na antenie gra.
4. Źle → odtworzenie (§5) i tymczasowo konkretna wersja: `LEGACY_IMAGE=ghcr.io/ersatztv/legacy:v<poprzednia>` w `.env`.

## 5. Odtworzenie

Po awarii dysku, nieudanej aktualizacji albo przeniesieniu na nowy serwer:

```bash
docker compose down
mv "$ETV_CONFIG" "$ETV_CONFIG.zepsute"           # nie kasuj od razu
tar -C "$(dirname "$ETV_CONFIG")" -xzf etv-config-<data>.tgz
# .env z kopią tokenów; kopia ze starszego legacy niż najnowsze – nowsze ją zmigruje, nowszej niż
# najnowsze nie ma; po nieudanej aktualizacji: LEGACY_IMAGE=ghcr.io/ersatztv/legacy:v<wersja kopii>
docker compose up -d
```

Potem: Diagnostyka (most zielony), **Playouty → Resetuj wszystkie** (ramówka od chwili obecnej), **Biblioteki → Skanuj** (gdy media się zmieniły). Ścieżki mediów w kontenerze (`/media`) muszą być te same co przed awarią.

## 6. Awarie

| Objaw | Sprawdź |
|---|---|
| most czerwony | `docker compose ps`, `docker compose logs --tail 200 ersatztv`; `VIZRS_BRIDGE_TOKEN` w `.env` ten sam dla obu usług; po restarcie legacy startuje do minuty |
| `commandsMissing` niepuste (Diagnostyka) | najnowsze legacy zmieniło komendy – wersja w Diagnostyce; tymczasowo `LEGACY_IMAGE=ghcr.io/ersatztv/legacy:v<poprzednia>` w `.env`, zgłoś do poprawki mostu |
| panel nie startuje | `docker compose logs live` – brak/za krótki token (min. 16 znaków, litery, cyfry, `-._~`) |
| widzowie bez strumieni | `curl -I http://<serwer>:8409/iptv/channels.m3u`; `docker compose logs proxy` |
| dysk pełny | `etv-config/ffmpeg-reports`, `logs`; Diagnostyka → konserwacja (segmenty HLS są w tmpfs `/transcode`, czyli w RAM – nie na dysku) |
| pamięć RAM | segmenty HLS kanałów w tmpfs `/transcode` (`docker compose exec ersatztv df -h /transcode`); legacy bez limitu sterty .NET – limit tylko na Ampere (nakładka CG Playout) |
| po aktualizacji / restarcie panelu okno logowania | zamierzone – sesje są w pamięci panelu (K1); zaloguj się ponownie |
| 429 przy logowaniu | odczekaj minutę; w dzienniku panelu »login: zły token (adres)« |
| potrzebne web GUI legacy (wyjątkowo) | tunel SSH albo tymczasowo `ports: ["127.0.0.1:8410:8409"]` przy `ersatztv` (DOKUMENTACJA §8); cofnij po pracy |

**Wymiana tokenu panelu** (np. odejście operatora): nowy `VIZRS_LIVE_TOKEN` w `.env`, `docker compose up -d live` – restart panelu unieważnia wszystkie sesje (magazyn sesji jest w pamięci, DOKUMENTACJA §3); skrypty z `Authorization: Bearer` muszą dostać nowy token. Token mostu: nowy `VIZRS_BRIDGE_TOKEN`, `docker compose up -d` (obie usługi).

## 7. Wydanie nowej wersji (opiekun repozytorium)

1. `tests/gate.sh` zielony; wersja w `server/Cargo.toml`, `bridge/Bridge.cs`, `bridge/VizRS.Bridge.csproj`; CHANGELOG; tagi w `.env.example` i `compose.yml`.
2. `git tag v<wersja> && git push origin main v<wersja>` – `.github/workflows/release.yml` przechodzi bramkę, publikuje oba obrazy (amd64 + arm64, `:<wersja>` i `:latest`) na `ghcr.io/sdorpl/…` i zakłada wydanie na GitHubie z binarkami panelu i sumami (uprawnienia `contents: write`, `packages: write`, `GITHUB_TOKEN`). Próba bez znacznika: Actions → Release → Run workflow z wersją – szkic wydania, obrazy tylko z tagiem wersji; zbędny szkic – Actions → Usuń szkic wydania.
3. Pierwsze wydanie: w ustawieniach pakietów GitHub ustaw `vizrs-live` i `vizrs-live-legacy` jako publiczne (albo `docker login ghcr.io` na serwerze).

Bez dostępu do Actions – ręcznie: `docker build -f docker/live.Dockerfile -t ghcr.io/sdorpl/vizrs-live:<w> .`, `docker build -f docker/legacy-bridge.Dockerfile -t ghcr.io/sdorpl/vizrs-live-legacy:<w>-legacy<l> .`, `docker login ghcr.io`, `docker push …`.
