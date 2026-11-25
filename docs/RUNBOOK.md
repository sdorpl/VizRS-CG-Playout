# VizRS CG Playout – runbook administratora

Wdrożenie, kopie, aktualizacja legacy, odtworzenie. Zakres: cały produkt (sidecar, silnik grafiki VizRS, panel kanału i VizRS Bridge). Praca operatora: [INSTRUKCJA.md](INSTRUKCJA.md); szczegóły techniczne i architektura: [DOKUMENTACJA.md](DOKUMENTACJA.md); specyfikacja interfejsów HTTP: [API.md](API.md).

## 1. Składniki i porty

Wdrożenie opiera się na głównym pliku `docker-compose.yml` w korzeniu repozytorium oraz nakładce `docker-compose.live.yml`:

| Usługa (`docker-compose.live.yml`) | Obraz | Port na zewnątrz |
|---|---|---|
| `live` – panel | `ghcr.io/sdorpl/vizrs-live:latest` (`LIVE_IMAGE`) | 8090 – operatorzy (w produkcji zalecane za proxy TLS) |
| `ersatztv` – legacy z mostem | `ghcr.io/sdorpl/vizrs-live-legacy:latest` (`LIVE_LEGACY_IMAGE`); lokalnie budowany nakładką `panel/compose.build.yml` na `ghcr.io/ersatztv/legacy:latest` + VizRS Bridge | brak (`expose`, sieć Dockera) |
| `proxy` – Caddy | `caddy:alpine` | 8409 – widzowie: strumienie, M3U, XMLTV, HDHomeRun |

W stosie z grafiką VizRS (`COMPOSE_PROFILES=grafika`) dochodzą usługi z `docker-compose.yml`:
- `seed` – jednorazowa inicjalizacja wolumenów danymi startowymi,
- `vizrs` – silnik grafiki VizRS (port 8788),
- `cg` – sidecar grafiki i Live API (port 7700),
- oraz z profilem `ingest` (`COMPOSE_PROFILES=grafika,ingest`): usługa `ingest` (RTMP/SRT).

Dane: wolumen `ersatztv-config` (albo katalog z `ERSATZTV_CONFIG`; cała baza i stan legacy) i `MEDIA_DIR` / `MEDIA_MOUNT` (media, tylko do odczytu). Panel nie trzyma żadnych własnych danych na dysku (sesje w RAM).

> [!NOTE]
> Szczegółowy opis konfiguracji Dockera dla sidecara, synchronizacji z ErsatzTV, `userland-proxy`, limitów potoków pamięci oraz danych startowych `seed/` znajduje się w [DOKUMENTACJA.md §4](DOKUMENTACJA.md#4-uruchamianie-w-dockerze).

## 2. Wdrożenie

1. Docker z Compose v2 i repozytorium (`docker-compose.yml`, `docker-compose.live.yml` w korzeniu).
2. Plik `.env` w korzeniu repozytorium (wzór zmiennych panelu: `panel/.env.example`); wygeneruj token mostu `head -c 24 /dev/urandom | base64 | tr -d '/+='` → `VIZRS_BRIDGE_TOKEN`; `VIZRS_LIVE_TOKEN` (token awaryjny panelu, inny niż mostu) – opcjonalny, potrzebny dla okna AppImage i skryptów. Konta operatorów zakłada się w panelu: po pierwszym starcie otwórz panel i wpisz kod z `docker compose logs live` („kod pierwszej konfiguracji”) – [DOKUMENTACJA §11](DOKUMENTACJA.md#11-panel-vizrs-live). Ustaw `MEDIA_DIR` (i `MEDIA_MOUNT`), `TZ`, `VIZRS_LIVE_STREAM_URL=http://<serwer>:8409` (adres dla widzów). Jeśli stos ma nadawać z grafiką VizRS, ustaw `COMPOSE_PROFILES=grafika` (lub `COMPOSE_PROFILES=grafika,ingest`). `chmod 600 .env`.
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

> [!NOTE]
> Szczegóły uruchamiania stosów na platformie Oracle Cloud Ampere A1 (`docker-compose.ampere.yml`), specyfika konfiguracji stosów s1 i s2, live ingest w Dockerze oraz automatyczna aktualizacja wydań przez plik `.release` i workflow `stack-refresh` opisane są w [DOKUMENTACJA.md §4](DOKUMENTACJA.md#4-uruchamianie-w-dockerze).

> [!WARNING]
> **Ważne ostrzeżenie dla Portainera (stosy z grafiką):** Stosy w Portainerze, które mają grafikę VizRS, **MUSZĄ** mieć ustawioną zmienną środowiskową stosu `COMPOSE_PROFILES=grafika` (lub `COMPOSE_PROFILES=grafika,ingest` dla live ingest) **PRZED** pierwszym wdrożeniem wersji z profilami compose. Z dawną wartością `COMPOSE_PROFILES=ingest` compose odrzuca nowy plik (`ingest` zależy od wyłączonego `cg`) i stos przestaje się aktualizować; z pustą wartością compose pomija `vizrs` i `cg`, co zdejmuje grafikę z anteny. Wartość `grafika,ingest` działa też ze starszą wersją compose (nieznany profil jest ignorowany), więc można ją ustawić wcześniej.

**Aplikacja okienna i dostęp ze stacji operatora.** Aplikacja okienna (`VizRS-CG-Playout-<wersja>-x86_64.AppImage`) to wyłącznie natywny klient panelu działającego na serwerze produkcyjnym (nie zawiera panelu lokalnego). Port 8090 nie powinien być wystawiony bezpośrednio na zewnątrz. Dostęp ze stacji operatora realizowany jest przez proxy nginx z własną domeną i certyfikatem TLS na serwerze produkcyjnym. Operator w oknie „VizRS CG Playout – połącz z panelem” wpisuje `https://<domena serwera>` i token panelu (`VIZRS_LIVE_TOKEN` – okno łączy się dalej tokenem awaryjnym, nie kontem).

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
   - `VIZRS_LIVE_TOKEN` (opcjonalny token awaryjny: okno AppImage, skrypty, pierwsze konto zamiast kodu z logu; operatorzy logują się kontami z panelu),
   - `VIZRS_PANEL_TOKEN` (token live API sidecara),
   - `VIZRS_LIVE_STREAM_URL=http://<serwer>:8409`.
4. **Deploy the stack** / **Pull and redeploy**. Przed pierwszym razem zrób kopię `etv-config` (§3) i porównaj wersję legacy w obrazie z mostem z działającą.

**Caddyfile proxy w Portainerze:** Portainer przy stosach z gita nie montuje plików z repozytorium na hosta. Z tego powodu Caddyfile usługi `proxy` jest wpisany bezpośrednio w sekcję `configs:` nakładki `docker-compose.live.yml`. Zgodność tej kopii ze źródłowym plikiem `panel/docker/proxy.Caddyfile` weryfikuje skrypt `panel/scripts/check-caddy.sh` w CI (`panel-ci.yml`) oraz w `panel/tests/deploy.sh` (zastąpił dawny `check-cg-overlay.sh` i `panel/compose.cg-playout.yml`).

**Media w panelu.** Usługa `live` montuje media tak jak legacy (`${MEDIA_DIR:-seed-media}:${MEDIA_MOUNT:-/media}:ro`) – Przeglądarka gra pliki lokalne bez obciążania silnika legacy ([DOKUMENTACJA.md §11](DOKUMENTACJA.md#11-panel-vizrs-live) oraz [API.md §2](API.md)). Inna ścieżka mediów w legacy = ta sama w panelu. Pozycje Jellyfin nie potrzebują montowania (plik z Jellyfin przez serwer panelu).

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

1. **Test obrazu bez GPU**: uruchom skrypt `panel/tests/hwaccel.sh [obraz]` (domyślnie `vizrs-live-legacy:dev`). Skrypt weryfikuje obecność akceleracji, koderów i sterowników wewnątrz kontenera i zwraca kod ≠ 0 przy jakimkolwiek braku (używany w CI i przy wydaniach).
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
- Konta panelu: wolumen `live-data` (`users.json`, `sessions.json` – skróty haseł, nie hasła); kopia np. `docker run --rm -v <prefiks>_live-data:/d -v "$PWD":/b alpine cp /d/users.json /b/` (prawa 0600 zachowaj).
- W `etv-config` jest też katalog mostu `vizrs-bridge/` (klatkaż profili kodowania, `framerates.json`) – kopia całego wolumenu go obejmuje.

## 4. Aktualizacja

> [!WARNING]
> **Portainer:** Przed aktualizacją stosu z grafiką upewnij się, że w zmiennych środowiskowych stosu ustawione jest `COMPOSE_PROFILES=grafika` (lub `COMPOSE_PROFILES=grafika,ingest`). Bez niej nowa wersja nie wejdzie albo zdejmie grafikę z anteny (§2).

**Panel:** obrazy panelu i legacy z mostem idą z `:latest` (decyzja 2026-11-16), jak `vizrs-cg`: nowe wydanie przesuwa `:latest`, `stack-refresh` zapisuje je w `.release`, a Portainer przeładowuje stos z „Re-pull image”. Obraz legacy z mostem w wydaniu jest budowany na najnowszym `ghcr.io/ersatztv/legacy`, więc wydanie może przynieść nowsze legacy, które migruje bazę – przed wydaniem (albo przy każdym tagu bez łącznika) kopia `etv-config` (§3). Inną wersję daje zmienna stosu `LIVE_IMAGE=ghcr.io/sdorpl/vizrs-live:<wersja>`.

**Legacy** – zawsze najnowsze `ghcr.io/ersatztv/legacy` (decyzja 2026-11-07, jak w CG Playout): każde `docker compose up -d` pobiera nowsze legacy, jeśli jest, i buduje na nim most. Nowsze legacy migruje bazę bez pytania, więc:
1. Przed `up`, które może przynieść nowe legacy, zrób kopię `etv-config` (§3). Wcześniejsze sprawdzenie: `tests/stack.sh up && tests/stack.sh seed && tests/stack.sh test` na maszynie testowej ([DOKUMENTACJA.md §14](DOKUMENTACJA.md#14-aktualizacja-legacy)) – stos testowy też bierze najnowsze.
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
| panel nie startuje | `docker compose logs live` – za krótki token (min. 16 znaków, litery, cyfry, `-._~`) albo poza 127.0.0.1 ani tokenu, ani `VIZRS_LIVE_DATA` |
| pierwsze uruchomienie – jaki kod? | `docker compose logs live` (Portainer: kontener `live` → Logs): „kod pierwszej konfiguracji: …”; po restarcie panelu kod jest nowy; zamiast kodu – `VIZRS_LIVE_TOKEN` |
| w logu „KONTA WYŁĄCZONE” | plik `users.json` na wolumenie `live-data` uszkodzony albo wolumen tylko do odczytu – panel loguje tylko tokenem awaryjnym; przywróć plik z kopii (§3) i `docker compose restart live` |
| Trio nie pokazuje loginu i hasła | w panelu (Użytkownicy) włączone „logowanie w pakiecie”? VizRS z obsługą kont panelu i `VIZRS_PANEL_LOGIN` (nakładka `docker-compose.live.yml`, profil `grafika`); stan przełącznika VizRS pamięta 10 s |
| nikt nie pamięta hasła administratora | zaloguj się tokenem awaryjnym (przycisk „Token awaryjny”) i w Użytkownicy ustaw nowe hasło; stos bez tokenu: dopisz `VIZRS_LIVE_TOKEN` w zmiennych stosu i zaktualizuj stos |
| widzowie bez strumieni | `curl -I http://<serwer>:8409/iptv/channels.m3u`; `docker compose logs proxy` |
| dysk pełny | `etv-config/ffmpeg-reports`, `logs`; Diagnostyka → konserwacja (segmenty HLS są w tmpfs `/transcode`, czyli w RAM – nie na dysku) |
| pamięć RAM | segmenty HLS kanałów w tmpfs `/transcode` (`docker compose exec ersatztv df -h /transcode`); legacy bez limitu sterty .NET – limit tylko na Ampere (nakładka CG Playout) |
| po aktualizacji / restarcie panelu okno logowania | panel bez wolumenu `live-data` (stara nakładka compose) trzyma sesje w pamięci – zaktualizuj stos; z wolumenem sesje przeżywają restart |
| 429 przy logowaniu | odczekaj minutę; w dzienniku panelu »login: zły token (adres)« albo »login: złe hasło albo login … (adres)« |
| potrzebne web GUI legacy (wyjątkowo) | tunel SSH albo tymczasowo `ports: ["127.0.0.1:8410:8409"]` przy `ersatztv` ([DOKUMENTACJA.md §15](DOKUMENTACJA.md#15-zamknięcie-web-gui-legacy-r10)); cofnij po pracy |

**Odejście operatora:** System → Użytkownicy → „Usuń” (konto i jego sesje znikają od razu) – bez zmian w `.env`. **Wymiana tokenu awaryjnego:** nowy `VIZRS_LIVE_TOKEN` w `.env`, `docker compose up -d live`; sesje założone tokenem trwają do wylogowania albo 30 dni (z wolumenem `live-data` przeżywają restart – ich koniec: usuń `sessions.json` z wolumenu przed restartem); skrypty z `Authorization: Bearer` muszą dostać nowy token. Token mostu: nowy `VIZRS_BRIDGE_TOKEN`, `docker compose up -d` (obie usługi).

## 7. Wydanie nowej wersji (opiekun repozytorium)

Od połączenia P2 panel kanału, most VizRS Bridge oraz aplikacja okienna są wydawane wspólnie z sidecarem w jednym workflow wydania produktu `.github/workflows/release.yml`.

### Obrazy i wersje

| Obraz | Tag | Z czego |
|---|---|---|
| `ghcr.io/sdorpl/vizrs-live` | `<wersja>` i `latest`, amd64 + arm64 | `panel/docker/live.Dockerfile` (statyczna binarka musl w `scratch`, kompilacja z `-C target-cpu=neoverse-n1` na arm64) |
| `ghcr.io/sdorpl/vizrs-live-legacy` | `<wersja>` i `<wersja>-legacy<legacy>`, amd64 + arm64 | `panel/docker/legacy-bridge.Dockerfile` na `LEGACY_IMAGE` z `.env.example` (domyślnie najnowszy `ghcr.io/ersatztv/legacy:latest`); akceleracja Intel VAAPI/QSV, AMD VAAPI i NVIDIA NVENC z obrazu upstream weryfikowana przez `panel/tests/hwaccel.sh` przed publikacją |
| `ghcr.io/sdorpl/vizrs-cg`, `vizrs-seed` | `<wersja>` i `latest`, amd64 + arm64 | sidecar i dane startowe w korzeniu repozytorium |
| `caddy` (proxy) | `alpine` (zawsze najnowszy) | obraz oficjalny |

W Portainerze obowiązują wartości domyślne `docker-compose.live.yml`: panel i legacy z mostem z `:latest` (`LIVE_IMAGE`, `LIVE_LEGACY_IMAGE`), przesuwane wydaniem produktu. Dla lokalnego stosu (`panel/.env.example`, `panel/tests/deploy.sh`) domyślne to najnowsze wydania: `LIVE_IMAGE=ghcr.io/sdorpl/vizrs-live:latest`, `PROXY_IMAGE=caddy:alpine`; konkretną wersję daje `LIVE_IMAGE=…:<wersja>` w `.env`. Legacy – od 2026-11-07 zawsze najnowsze ([DOKUMENTACJA.md §14](DOKUMENTACJA.md#14-aktualizacja-legacy)): `LEGACY_IMAGE=ghcr.io/ersatztv/legacy:latest`, `LIVE_LEGACY_IMAGE=vizrs-live-legacy:local` (lokalny obraz z mostem, budowany nakładką `panel/compose.build.yml`); inna wersja: `LEGACY_IMAGE=…` w `.env`.

### Procedura wydania

1. `panel/tests/gate.sh` i bramka korzenia zielone; wspólna wersja produktu we wszystkich manifestach:
   - `Cargo.toml` w korzeniu repozytorium,
   - `panel/server/Cargo.toml`,
   - `panel/desktop/Cargo.toml`,
   - `panel/desktop/tauri.conf.json`,
   - `panel/bridge/VizRS.Bridge.csproj` i stała `Version` w `panel/bridge/Bridge.cs`;
   - wpis w `CHANGELOG.md` w sekcji z numerem wersji.
2. `git tag v<wersja> && git push origin main v<wersja>` – workflow `.github/workflows/release.yml`:
   - sprawdza zgodność jednej wersji produktu we wszystkich manifestach (także `Version` w `Bridge.cs`);
   - wywołuje CI tego commita (`ci.yml` i `panel-ci.yml`: testy sidecara, bramka panelu `panel/tests/gate.sh`, weryfikacja akceleracji sprzętowej `panel/tests/hwaccel.sh`, okno, AppImage) – obrazy i wydanie powstają dopiero po zielonym CI;
   - buduje binarki panelu `VizRS-CG-Playout-panel-<wersja>-linux-{x86_64,arm64}.tar.gz` (statyczne musl) oraz sidecara;
   - buduje obrazy Dockera (`vizrs-cg`, `vizrs-seed`, `vizrs-live`, `vizrs-live-legacy`) na amd64 i arm64;
   - dołącza aplikację okienną (Tauri 2) z CI panelu: `VizRS-CG-Playout-<wersja>-x86_64.AppImage` (natywny klient panelu na serwerze; wydania: https://github.com/sdorpl/VizRS-CG-Playout/releases);
   - zakłada wydanie na GitHubie z plikami i sumami `SHA256SUMS.txt`.
   Próba bez znacznika: Actions → Release → Run workflow z wersją – szkic wydania (obrazy tylko z numerem, bez `:latest`); z `publish` – od razu wydanie i znacznik `v<wersja>` na wybranym commicie.
3. Pakiety `vizrs-live` i `vizrs-live-legacy` na GitHubie ustawione jako publiczne.
4. Bez dostępu do Actions – ręczna budowa:
   ```bash
   docker build -f panel/docker/live.Dockerfile -t ghcr.io/sdorpl/vizrs-live:<wersja> panel
   docker build -f panel/docker/legacy-bridge.Dockerfile -t ghcr.io/sdorpl/vizrs-live-legacy:<wersja>-legacy<legacy> panel
   docker login ghcr.io
   docker push ghcr.io/sdorpl/vizrs-live:<wersja>
   docker push ghcr.io/sdorpl/vizrs-live-legacy:<wersja>-legacy<legacy>
   ```

### Decyzje dotyczące wydań

- **Tag obrazu legacy** niesie wersję panelu oraz wersję upstream legacy (`<wersja>-legacy<legacy>`), co precyzuje wersję bazową.
- **Wydanie z tagu bez łącznika** przesuwa tag `:latest`, a wersja z łącznikiem (`v26.11.11-rc1`) to wydanie wstępne bez przesuwania `:latest`.
- **TLS**: panel i proxy działają po HTTP; dostęp z zewnątrz – za odwrotnym proxy z TLS (np. nginx, §2).
- **Zmienne**: nazwy obrazów oraz zmienne środowiskowe `VIZRS_LIVE_*` pozostają bez zmian (stosy produkcyjne ich używają).

### Przegląd bezpieczeństwa

| Obszar | Stan | Zmiana w 1.0 |
|---|---|---|
| tokeny | panel i most: min. 16 znaków, porównanie w stałym czasie (`eq_ct`); most bez tokenu = 503; panel poza 127.0.0.1 nie startuje bez tokenu albo kont; konta: hasła PBKDF2-HMAC-SHA256 (310 000 iteracji, sól), sesje w pliku jako skrót SHA-256, pliki 0600, pierwsze konto tylko z kodem z logu albo tokenem; token CG zostaje w serwerze | **limit logowań na IP**: 10 błędów w oknie 60 s → 429 dla adresu (bezpiecznik łączny 100/60 s, pamięć do 4096 IP, bez zaufania do `X-Forwarded-For`), wpis w dzienniku: `login: zły token (<IP>)` |
| sesja | ciasteczko `HttpOnly`, `SameSite=Strict`, 30 dni bezczynności; wartość = losowy identyfikator sesji z magazynu w pamięci (K1), „Wyloguj” unieważnia sesję, restart panelu – wszystkie | `Secure` za proxy TLS |
| CSRF | `X-VizRS-Live: 1` na każde żądanie zmieniające (też logowanie, `PUT`, `DELETE`), `SameSite=Strict` | e2e na osobnym panelu z tokenem |
| CSP / XSS | tekst do DOM tylko `textContent`, brak `innerHTML`/`eval` w `server/web/` (sprawdzone `grep`), bez inline | CSP na **wszystkich** odpowiedziach (wcześniej tylko wbudowane UI), dodane `object-src`, `base-uri`, `form-action`, `frame-ancestors` |
| limity | nagłówki 16 KB, treść 1 MB (wgrywanie 10 MB, tylko PNG/JPEG/GIF/WebP z sygnaturą), 64 połączenia, czas 15 s, bez `Transfer-Encoding` | – |
| most | biała lista komend, 403 spoza niej; bez `Get…ConnectionParameters`, `TryCompletePlexPinFlow`, `RebuildSearchIndex`; most tylko w sieci Dockera, przez proxy 403 | – |
| sekrety (R8) | maskowanie `…apikey/token/password/secret` w odpowiedziach komend i CG | – |
| proxy (R10) | na zewnątrz tylko `GET/HEAD` strumieni, `/artwork/*`, HDHomeRun | obraz `caddy:alpine` (najnowszy) |
| obrazy | panel: `scratch`, użytkownik 10001, statyczna binarka; legacy: obraz upstream bez zmian + 2 pliki | panel i proxy – najnowsze wydania (`latest`, od 2026-11-09); legacy zawsze najnowsze ([DOKUMENTACJA.md §14](DOKUMENTACJA.md#14-aktualizacja-legacy)) – kopia `etv-config` przed `up` |

Świadomie pozostawione (ryzyko przyjęte):
- `frame-src` przy `--vizrs-url` w postaci samego portu dopuszcza ten port na dowolnym hoście (CSP nie wyraża „host panelu”; podaj pełny adres, by zawęzić);
- token wyjścia VizRS (`--vizrs-output-token`) jest widoczny w przeglądarce zalogowanego operatora – daje tylko podgląd wyjścia, jak adres w OBS, bez sterowania (VizRS DOKUMENTACJA „Token wyjścia”);
- bezpiecznik łączny limitu logowań (100 błędów/60 s) przy rozproszonym ataku oraz wspólny adres IP klientów za proxy/NAT lub przy userland-proxy Dockera (brak zaufania do `X-Forwarded-For`) mogą zablokować logowanie innym (operator z ważnym ciasteczkiem pracuje dalej);
- legacy w kontenerze działa jako root (obraz upstream – bez zmian legacy).
