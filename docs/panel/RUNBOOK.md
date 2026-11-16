# Panel VizRS CG Playout – runbook administratora

Wdrożenie, kopie, aktualizacja legacy, odtworzenie. Praca operatora: [INSTRUKCJA.md](INSTRUKCJA.md); budowa i API: [DOKUMENTACJA.md](DOKUMENTACJA.md).

## 1. Składniki i porty

Wdrożenie opiera się na głównym pliku `docker-compose.yml` w korzeniu repozytorium oraz nakładce `docker-compose.live.yml`:

| Usługa (`docker-compose.live.yml`) | Obraz | Port na zewnątrz |
|---|---|---|
| `live` – panel | `ghcr.io/sdorpl/vizrs-live:<wersja>` – przypięta w nakładce (`LIVE_IMAGE`, dziś 26.11.8) | 8090 – operatorzy (w produkcji zalecane za proxy TLS) |
| `ersatztv` – legacy z mostem | `ghcr.io/sdorpl/vizrs-live-legacy:<wersja>` – przypięta w nakładce (`LIVE_LEGACY_IMAGE`, dziś 26.11.8); lokalnie budowany nakładką `panel/compose.build.yml` na `ghcr.io/ersatztv/legacy:latest` + VizRS Bridge | brak (`expose`, sieć Dockera) |
| `proxy` – Caddy | `caddy:alpine` | 8409 – widzowie: strumienie, M3U, XMLTV, HDHomeRun |

W stosie z grafiką VizRS (`COMPOSE_PROFILES=grafika`) dochodzą usługi z `docker-compose.yml`:
- `seed` – jednorazowa inicjalizacja wolumenów danymi startowymi,
- `vizrs` – silnik grafiki VizRS (port 8788),
- `cg` – sidecar grafiki i Live API (port 7700),
- oraz z profilem `ingest` (`COMPOSE_PROFILES=grafika,ingest`): usługa `ingest` (RTMP/SRT).

Dane: wolumen `ersatztv-config` (albo katalog z `ERSATZTV_CONFIG`; cała baza i stan legacy) i `MEDIA_DIR` / `MEDIA_MOUNT` (media, tylko do odczytu). Panel nie trzyma żadnych własnych danych na dysku (sesje w RAM).

## 2. Wdrożenie

1. Docker z Compose v2 i repozytorium (`docker-compose.yml`, `docker-compose.live.yml` w korzeniu).
2. Plik `.env` w korzeniu repozytorium (wzór zmiennych panelu: `panel/.env.example`); wygeneruj dwa **różne** tokeny: `head -c 24 /dev/urandom | base64 | tr -d '/+='` → `VIZRS_BRIDGE_TOKEN`, `VIZRS_LIVE_TOKEN`. Ustaw `MEDIA_DIR` (i `MEDIA_MOUNT`), `TZ`, `VIZRS_LIVE_STREAM_URL=http://<serwer>:8409` (adres dla widzów). Jeśli stos ma nadawać z grafiką VizRS, ustaw `COMPOSE_PROFILES=grafika` (lub `COMPOSE_PROFILES=grafika,ingest`). `chmod 600 .env`.
3. Uruchomienie stosu:
   ```bash
   # Z grafiką VizRS (emisja):
   COMPOSE_PROFILES=grafika docker compose -f docker-compose.yml -f docker-compose.live.yml up -d

   # Z grafiką VizRS i live ingest:
   COMPOSE_PROFILES=grafika,ingest docker compose -f docker-compose.yml -f docker-compose.live.yml up -d

   # Bez grafiki (sam ErsatzTV z panelem):
   docker compose -f docker-compose.yml -f docker-compose.live.yml up -d

   # Lokalna budowa obrazów panelu (np. przy pracy nad kodem / deploy.sh):
   COMPOSE_PROFILES=grafika docker compose -f docker-compose.yml -f docker-compose.live.yml -f panel/compose.build.yml up -d
   ```
4. Sprawdź: `docker compose ps` (wszystko `healthy`/`running`), panel `http://<serwer>:8090/` → zaloguj → **Diagnostyka**: most zielony, „Web GUI legacy dla widzów” = zamknięte. Z innej maszyny: `curl -I http://<serwer>:8409/` → 403, `curl -I http://<serwer>:8409/iptv/channels.m3u` → 200.
5. Pierwsza konfiguracja (biblioteka, kanał, harmonogram, playout) – w panelu; legacy nie wymaga kreatora.

> [!WARNING]
> **Ważne ostrzeżenie dla Portainera (stosy z grafiką):** Stosy w Portainerze, które mają grafikę VizRS, **MUSZĄ** mieć ustawioną zmienną środowiskową stosu `COMPOSE_PROFILES=grafika` (lub `COMPOSE_PROFILES=grafika,ingest` dla live ingest) **PRZED** pierwszym wdrożeniem wersji z profilami compose. Z dawną wartością `COMPOSE_PROFILES=ingest` compose odrzuca nowy plik (`ingest` zależy od wyłączonego `cg`) i stos przestaje się aktualizować; z pustą wartością compose pomija `vizrs` i `cg`, co zdejmuje grafikę z anteny. Wartość `grafika,ingest` działa też ze starszą wersją compose (nieznany profil jest ignorowany), więc można ją ustawić wcześniej.

**Aplikacja okienna i dostęp ze stacji operatora.** Aplikacja okienna (`VizRS-CG-Playout-<wersja>-x86_64.AppImage`) to wyłącznie natywny klient panelu działającego na serwerze produkcyjnym (nie zawiera panelu lokalnego). Port 8090 nie powinien być wystawiony bezpośrednio na zewnątrz. Dostęp ze stacji operatora realizowany jest przez proxy nginx z własną domeną i certyfikatem TLS na serwerze produkcyjnym. Operator w oknie „VizRS CG Playout – połącz z panelem” wpisuje `https://<domena serwera>` i token panelu (`VIZRS_LIVE_TOKEN`).

**Przykład konfiguracji nginx reverse proxy przed panelem:**
```nginx
# Przykład konfiguracji nginx przed panelem (serwer z TLS)
server {
    listen 443 ssl http2;
    server_name panel.twojadomena.pl;

    # Certyfikaty TLS serwera produkcyjnego (np. certbot / Let's Encrypt)
    ssl_certificate /etc/letsencrypt/live/panel.twojadomena.pl/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/panel.twojadomena.pl/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8090;

        # Nagłówek Authorization jest domyślnie przekazywany przez nginx – nie nadpisuj go
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # Podgląd HLS i długie odpowiedzi panelu – bez buforowania:
        proxy_http_version 1.1;
        proxy_buffering off;
        proxy_read_timeout 3600s;
        client_max_body_size 10m;   # wgrywanie logo i znaków wodnych (limit panelu: 10 MB)
    }
}
```
> [!NOTE]
> Powyższa konfiguracja jest przykładem; domena oraz certyfikaty TLS należą do konfiguracji serwera produkcyjnego.

**CG Playout (Live API).** W nakładce `docker-compose.live.yml` połączenie z sidecarem jest skonfigurowane przez sieć wewnętrzną Dockera (`http://cg:7700`) z tokenem `VIZRS_PANEL_TOKEN`. Bez profilu `grafika` panel działa nad samym ErsatzTV, a sidecar jest traktowany jako niedostępny (`cg` w nakładce ma `required: false`).

**VizRS CG System (opcjonalnie, podgląd grafiki i Trio).** `VIZRS_LIVE_VIZRS_URL` – adres VizRS widziany z przeglądarki operatora (nie z kontenera): `http://<serwer>:8788` albo sam port `:8788` (ten sam host co panel – zalecane, ciasteczko Trio działa wtedy także w ramce). VizRS z tokenem: `VIZRS_LIVE_VIZRS_OUTPUT_TOKEN` = `VIZRS_OUTPUT_TOKEN` VizRS (tylko odczyt). Anteny kanałów (`VIZRS_CHANNEL_BUSES=1` w CG Playout): `VIZRS_LIVE_VIZRS_CHANNELS=1`. Sprawdzenie: Grafika kanału → ramka pokazuje wyjście (nie „wymagany token”), Diagnostyka → Panel.

### 2a. Wdrożenie w Portainerze (GitOps) i konfiguracja proxy

W Portainerze (stos z GitHuba, np. s1):
1. **Repository → Compose path**: `docker-compose.yml`.
2. **Additional paths**: `docker-compose.live.yml` (na maszynach Ampere A1 po `docker-compose.ampere.yml`, z kartą NVIDIA po `docker-compose.nvidia.yml`).
3. **Environment variables**:
   - `COMPOSE_PROFILES=grafika` (lub `grafika,ingest` dla live ingest – **pamiętaj o ustawieniu przed wdrożeniem!**),
   - `VIZRS_BRIDGE_TOKEN` (token mostu),
   - `VIZRS_LIVE_TOKEN` (logowanie do panelu i okna AppImage),
   - `VIZRS_PANEL_TOKEN` (token live API sidecara),
   - `VIZRS_LIVE_STREAM_URL=http://<serwer>:8409`.
4. **Deploy the stack** / **Pull and redeploy**. Przed pierwszym razem zrób kopię `etv-config` (§3) i porównaj wersję legacy w obrazie z mostem z działającą.

**Caddyfile proxy w Portainerze:** Portainer przy stosach z gita nie montuje plików z repozytorium na hosta. Z tego powodu Caddyfile usługi `proxy` jest wpisany bezpośrednio w sekcję `configs:` nakładki `docker-compose.live.yml`. Zgodność tej kopii ze źródłowym plikiem `panel/docker/proxy.Caddyfile` weryfikuje skrypt `panel/scripts/check-caddy.sh` w CI (`panel-ci.yml`) oraz w `panel/tests/deploy.sh` (zastąpił dawny `check-cg-overlay.sh` i `panel/compose.cg-playout.yml`).

**Media w panelu.** Usługa `live` montuje media tak jak legacy (`${MEDIA_DIR:-seed-media}:${MEDIA_MOUNT:-/media}:ro`) – Przeglądarka gra pliki lokalne bez obciążania silnika legacy (DOKUMENTACJA §3 `/api/vod`). Inna ścieżka mediów w legacy = ta sama w panelu. Pozycje Jellyfin nie potrzebują montowania (plik z Jellyfin przez serwer panelu).

### 2b. Akceleracja sprzętowa (Intel, AMD, NVIDIA)

Kodowanie wideo wykonuje ffmpeg w procesie ErsatzTV legacy (silnik Next); obraz panelu `vizrs-live` (`scratch`) nie zawiera ffmpeg – akceleracja dotyczy wyłącznie usługi `ersatztv` (obraz `vizrs-live-legacy`, lokalnie `vizrs-live-legacy:local`).

Oficjalny obraz bazowy `ghcr.io/ersatztv/legacy` (Ubuntu 24.04, ffmpeg 8.1.2-etv, amd64) zawiera już komplet koderów i sterowników:
- **`ffmpeg -hwaccels`**: `vaapi`, `qsv`, `cuda`;
- **kodery**: `h264_vaapi`, `hevc_vaapi`, `av1_vaapi`, `h264_qsv`, `hevc_qsv`, `av1_qsv`, `h264_nvenc`, `hevc_nvenc`;
- **sterowniki**: Intel VAAPI `iHD` (od generacji Broadwell) i `i965` (starszy Intel), AMD VAAPI `radeonsi` (Mesa); Intel QSV (`libmfx-gen` / Media SDK); `libva`.
- **NVIDIA**: biblioteki sterownika (`libcuda`, `libnvidia-encode`) muszą odpowiadać wersji sterownika na hoście, dlatego nie mogą znajdować się wewnątrz obrazu – dostarcza je w locie NVIDIA Container Toolkit przez CDI.
- **AMD**: AMD AMF (sterownik zamknięty) nie jest używany; karty AMD działają przez otwarte sterowniki VAAPI (Mesa `radeonsi`).

Most VizRS Bridge nie ingeruje w ffmpeg ani system bazowy (obraz legacy bez zmian).

| Karta | Nakładka (korzeń repo) | Na hoście | W panelu (Profile kodowania → Akceleracja) |
|---|---|---|---|
| **Intel** (iGPU, Arc) | `docker-compose.vaapi.yml` | sterownik jądra `i915` albo `xe` (`/dev/dri/renderD128`) | **VAAPI** (urządzenie `/dev/dri/renderD128`, sterownik `iHD` lub `i965`) albo **QSV** |
| **AMD** (Radeon) | `docker-compose.vaapi.yml` | sterownik jądra `amdgpu` (`/dev/dri/renderD128`) | **VAAPI** (urządzenie `/dev/dri/renderD128`, sterownik `RadeonSI`) |
| **NVIDIA** | `docker-compose.nvidia.yml` | sterownik NVIDIA + `nvidia-container-toolkit` + CDI (`/var/run/cdi/nvidia.yaml`) | **NVIDIA** |

#### Wymagania na hoście i nakładki compose

1. **Intel i AMD – VAAPI / QSV (`docker-compose.vaapi.yml`)**:
   - Nakładka przekazuje urządzenie `${VAAPI_DEVICE:-/dev/dri/renderD128}:/dev/dri/renderD128` do kontenera `ersatztv`.
   - W przypadku kilku kart graficznych wskaż właściwy węzeł za pomocą zmiennej środowiskowej `VAAPI_DEVICE` (np. sprawdzając `ls -l /dev/dri/by-path`).
   - Proces legacy w kontenerze działa jako `root`, więc dodawanie użytkownika do grupy `render` na hoście nie jest wymagane.

2. **NVIDIA – NVENC / CUDA (`docker-compose.nvidia.yml`)**:
   - Ustawia `NVIDIA_DRIVER_CAPABILITIES=compute,video,utility` i przekazuje kartę przez mechanizm CDI (`driver: cdi`, `device_ids: ["nvidia.com/gpu=${NVIDIA_GPU:-all}"]`) – nie wymaga przestarzałego runtime `nvidia` w Dockerze.
   - Na hoście wymagany jest sterownik NVIDIA oraz pakiet `nvidia-container-toolkit`.
   - Wygeneruj specyfikację CDI: `sudo nvidia-ctk cdi generate --output=/var/run/cdi/nvidia.yaml`.
   - Sprawdź dostępność urządzeń: `nvidia-ctk cdi list`.
   - Przy kilku kartach NVIDIA można wybrać konkretną przez zmienną `NVIDIA_GPU` (numer np. `0` lub UUID `GPU-…`, domyślnie `all`).

#### Uruchomienie

- **Z kartą Intel / AMD (VAAPI / QSV):**
  ```bash
  COMPOSE_PROFILES=grafika docker compose -f docker-compose.yml -f docker-compose.vaapi.yml -f docker-compose.live.yml up -d
  ```
- **Z kartą NVIDIA (NVENC / CUDA):**
  ```bash
  COMPOSE_PROFILES=grafika docker compose -f docker-compose.yml -f docker-compose.nvidia.yml -f docker-compose.live.yml up -d
  ```
- **Lokalna budowa obrazów z GPU (np. `panel/tests/deploy.sh`):**
  ```bash
  COMPOSE_FILE=docker-compose.yml:docker-compose.nvidia.yml:docker-compose.live.yml:panel/compose.build.yml COMPOSE_PROFILES=grafika,ingest docker compose up -d
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

> [!WARNING]
> **Portainer:** Przed aktualizacją stosu z grafiką upewnij się, że w zmiennych środowiskowych stosu ustawione jest `COMPOSE_PROFILES=grafika` (lub `COMPOSE_PROFILES=grafika,ingest`). Bez niej nowa wersja nie wejdzie albo zdejmie grafikę z anteny (§2).

**Panel:** obrazy panelu i legacy z mostem są przypięte w `docker-compose.live.yml` (`LIVE_IMAGE`, `LIVE_LEGACY_IMAGE`); nowe wydanie = commit z nowymi tagami w nakładce na `main` (Portainer przeładuje stos). Inną wersję daje zmienna stosu `LIVE_IMAGE=ghcr.io/sdorpl/vizrs-live:<wersja>`.

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

1. `tests/gate.sh` zielony; wspólna wersja produktu w: `Cargo.toml` w korzeniu repozytorium, `panel/server/Cargo.toml`, `panel/desktop/Cargo.toml`, `panel/desktop/tauri.conf.json` oraz `panel/bridge/VizRS.Bridge.csproj` i stała `Version` w `panel/bridge/Bridge.cs`; CHANGELOG. Po publikacji obrazów – nowe tagi `LIVE_IMAGE` i `LIVE_LEGACY_IMAGE` w `docker-compose.live.yml` (osobny commit: wcześniej obrazów jeszcze nie ma w ghcr).
2. `git tag v<wersja> && git push origin main v<wersja>` – workflow `.github/workflows/release.yml` sprawdza zgodność wersji we wszystkich manifestach, uruchamia bramkę panelu (`panel/tests/gate.sh`) i test akceleracji (`panel/tests/hwaccel.sh`), publikuje obrazy Dockera (`vizrs-cg`, `vizrs-seed`, `vizrs-live`, `vizrs-live-legacy`), buduje binarki sidecara i panelu `VizRS-CG-Playout-panel-<wersja>-linux-{x86_64,arm64}.tar.gz`, buduje aplikację okienną `VizRS-CG-Playout-<wersja>-x86_64.AppImage` oraz zakłada wydanie na GitHubie z sumami `SHA256SUMS.txt`. Próba bez znacznika: Actions → Release → Run workflow z wersją – szkic wydania.
3. Pakiety `vizrs-live` i `vizrs-live-legacy` na GitHubie ustawione jako publiczne.

Bez dostępu do Actions – ręcznie: `docker build -f panel/docker/live.Dockerfile -t ghcr.io/sdorpl/vizrs-live:<w> panel`, `docker build -f panel/docker/legacy-bridge.Dockerfile -t ghcr.io/sdorpl/vizrs-live-legacy:<w>-legacy<l> panel`, `docker login ghcr.io`, `docker push …`.
