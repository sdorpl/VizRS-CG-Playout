# VizRS Live – instrukcja operatora

Dla osoby, która prowadzi kanał na co dzień. Wdrożenie i administracja: [RUNBOOK.md](RUNBOOK.md); szczegóły techniczne: [DOKUMENTACJA.md](DOKUMENTACJA.md).

Web GUI ErsatzTV legacy **nie jest potrzebne** i jest zamknięte – wszystko robi się w panelu.

## 1. Wejście do panelu

1. Otwórz `http://<serwer>:8090/` (adres poda administrator).
2. Wpisz **token panelu** (`VIZRS_LIVE_TOKEN`) i „Zaloguj”. Sesja trwa 30 dni w tej przeglądarce; „wyloguj” = wyczyść ciasteczka strony.
3. Po 10 nieudanych próbach w ciągu minuty panel przez minutę nie przyjmuje logowania („za dużo nieudanych logowań”) – odczekaj.

Pasek górny: **wybór kanału** (obok nazwy panelu – Na antenie, EPG, Playouty i Sloty pokazują wtedy tylko ten kanał; „Wszystkie kanały” wyłącza filtr; przeglądarka pamięta wybór), stan mostu do legacy (zielony = działa), licznik playoutów z błędem budowy (klik → Playouty) i stan CG Playout. Liczby w menu – ile jest kanałów, kolekcji, playoutów itd. Przycisk **☰** (lewy górny róg) zwija menu do samych ikon – więcej miejsca na ekran, nazwa pod kursorem; przeglądarka pamięta wybór. Na telefonie ☰ wysuwa menu z boku.

## 2. Codzienna praca

| Chcę… | Ekran | Jak |
|---|---|---|
| zobaczyć, co leci teraz i dalej | **Na antenie** | teraz / następnie z postępem, najbliższe pozycje każdego kanału; w nagłówku ile kanałów ma bieżącą pozycję, na karcie stan (NA ANTENIE / ZASTĘPCZY – przerwa w ramówce), grafika VizRS kanału z plakietką „NA ŻYWO”, gdy studio jest na antenie, i stan studia z CG Playout (sygnał, wstrzymane, najbliższy slot LIVE) |
| sprawdzić ramówkę i obraz wszystkich kanałów naraz | **Na antenie** | „Sprawdź ramówkę” (błędy na czerwono: dziury, koniec playoutu, pliki w koszu, LIVE bez wejścia), „Mozaika” – podglądy kanałów, klik w kafel = dźwięk |
| zdjąć albo wprowadzić pojedynczą stronę grafiki | **Na antenie** | przy pigułce warstwy „W10 · …”: „Dalej” / „Zdejmij”; „Wprowadź stronę…” – wybór strony VizRS i TAKE (gdy CG Playout to obsługuje) |
| sprawdzić, co poszło na antenę | **Dziennik emisji** | kanał, dzień, filtr; „Eksport CSV”; grafika wprowadzona lub zdjęta w Trio (także makrem albo z telefonu) ma operatora „VizRS (poza panelem)” |
| sprawdzić, co działo się z emisją (zrywy sygnału live, kanał na ramówce zamiast live, błędy ErsatzTV) | **Dziennik emisji** → źródło | „live ingest – odbiorniki” (czy nadawca się rozłączał), „przejęcia kanału” (tempo przebiegów live, „behind the clock” = live ustąpił ramówce, żeby kanał nie został za zegarem), „sidecar VizRS CG”, „ErsatzTV legacy” albo „wszystko na jednej osi czasu”; „od: uwaga” pokazuje same problemy; dni w UTC |
| zareagować na alarm | banner **ALARM** pod paskiem górnym | klik w alarm prowadzi do ekranu; „Dźwięk: wł.” – sygnał przy nowym alarmie; „Ukryj na 10 min” |
| zatrzymać, uruchomić albo zrestartować emisję kanału | **Na antenie**, karta kanału | **Uruchom emisję** (gdy kanał nie nadaje), **Restart emisji**, **Zatrzymaj emisję** (oba z potwierdzeniem); pigułka „emisja: …” pokazuje stan sesji legacy. Zatrzymany kanał ruszy znowu, gdy ktoś poprosi o strumień – na dłużej wyłącz kanał w **Kanałach** |
| zdjąć albo przywrócić całą grafikę | **Na antenie**, nagłówek | **Zdejmij grafikę** (z potwierdzeniem) zdejmuje grafikę VizRS ze wszystkich kanałów z animacją wyjścia; pigułka „GRAFIKA ZDJĘTA” i uwaga przypominają o tym, **Przywróć grafikę** wprowadza ją z animacją wejścia. Wymaga VizRS CG Playout |
| obejrzeć kanał | **Podgląd** (pasek górny) albo **Podgląd ▶** na karcie kanału w Na antenie | okno w prawym dolnym rogu, zostaje przy zmianie ekranów; obraz idzie równo z zegarem anteny, kilka sekund za nim (stan „NA ŻYWO”, w stopce „N s za anteną”; tyle też trwa, zanim TAKE widać w podglądzie); dźwięk domyślnie wyłączony („Dźwięk”), „Większy”, „Pełny” (pełny ekran), × zamyka. Gdy kanału nikt nie ogląda, start trwa do kilkunastu sekund |
| sprawdzić przewodnik | **Przewodnik EPG** | oś 4 h wszystkich kanałów |
| dodać / zmienić kanał | **Kanały** | „+ Nowy kanał”, formularz, „Załóż kanał”; zmiana → „Zapisz”; logo: wgraj obraz albo podaj adres |
| zmienić preset, profil H.264 albo odciążyć procesor / kartę kosztem jakości | **Profile kodowania** | panel „Koder (nakładka VizRS CG)” pod listą: profil H.264 dla wszystkich koderów (`main` dla starszych odbiorników; `high` – lepsza kompresja, NVENC bez ustawienia koduje w `main`); programowo – preset x264 (na Ampere A1 `veryfast`, przy kilku kanałach `superfast`) i opcje x264 (`bframes=0:subme=0` to ok. −20% procesora za −1,4 dB jakości); sprzętowo – preset NVENC (`p1` najszybszy … `p7` najlepszy, domyślnie `p4`), preset QSV, klatki B (`0` = najmniej pamięci karty). **Zapisz koder**; działa od następnej pozycji ramówki. Ustawienia z panelu zastępują .env serwera. Na serwerze bez karty (np. Ampere A1) panel pisze, że wszystko koduje się programowo – liczy się wtedy tylko profil H.264, preset i opcje x264; ustawienia sprzętowe są zwinięte i zostają bez zmian |
| ustawić klatkaż kanału (np. 25 kl./s) | **Profile kodowania** | pole „Klatkaż” w profilu kanału: stała wartość, „automatycznie” (najniższy z ramówki – jeden film 23,976 ustawi cały kanał na 23,976) albo „jak materiał”; działa od następnego startu emisji (**Restart emisji** w Na antenie) |
| zmienić numerację wielu kanałów | **Kanały** | „Numeracja…”, wpisz nowe numery (można zamieniać miejscami), „Zapisz numerację” |
| dodać nowe pliki | **Biblioteki** → „Skanuj” | pliki muszą leżeć w katalogu mediów serwera; po skanie widać je w **Przeglądarce** |
| obejrzeć film, odcinek albo audycję | **Przeglądarka** → klik w pozycję (serial → sezon → odcinek) | w karcie **Odtwarzanie**: „od” (np. 12:30), **▶ Odtwórz**, dalej pasek odtwarzacza (przewijanie, pełny ekran) albo „‹ 30 s” / „30 s ›”. Plik gra prosto z Jellyfin albo z biblioteki – bez legacy i bez kodowania. Gdy przeglądarka nie zna kodeka, panel sam przełącza na **podgląd przez legacy** (koduje po 30 s wybranym profilem, jedno takie odtwarzanie naraz w całym legacy; przy „kodowanie nie powiodło się” – profil bez akceleracji); można też przełączyć ręcznie. Filmy i seriale mają okładki w tabeli i w karcie |
| zgrupować treść | **Kolekcje** (zwykłe, inteligentne, wielo-, powtórki), **Playlisty** | treść dobiera się „wyborem treści” z wyszukiwaniem; w **Przeglądarce** zapytanie można od razu zapisać jako kolekcję inteligentną, a wyniki oglądać jako kafle; kolekcji z pigułką **CG Playout** (`LIVE · …`) nie zmienia się tutaj – prowadzi je audycja na ekranie **Audycje LIVE** |
| wczytać ramówkę z systemu emisyjnego (CSV/XML) | **Playlisty** | „Import ramówki z pliku…”, wybierz plik i grupę, „Dalej”, sprawdź dopasowanie (żółte – wybierz pozycję, czerwone – pominięte), „Utwórz playlistę”; potem użyj jej w harmonogramie |
| ułożyć ramówkę klasyczną | **Harmonogramy** | pozycje: start stały (godzina) albo dynamiczny, tryb (jedna, kilka, czas, do pełna), treść; „Podgląd 24 h” pokazuje szkielet doby z godzin stałych; „Zapisz” zapisuje całą listę |
| ułożyć ramówkę blokową | **Bloki i szablony**, **Wystrój (deco)**, potem **Playouty** → kalendarz tygodnia | blok = treść na N minut; szablon dnia = bloki o godzinach; kalendarz = szablony na dni. W kalendarzu tygodnia domyślna siatka godziny × dni pokazuje bloki w stałych kolorach, deco z prawej strony kolumny i czerwoną linię „teraz”; pigułki nad siatką podsumowują przypisania szablonów, a kliknięcie bloku pokazuje pod siatką jego pozycje i wystrój. Przełącznik „Siatka” / „Paski” w nagłówku pozwala wrócić do widoku osi 24 h (wybór pamięta przeglądarka). |
| podpiąć ramówkę do kanału | **Playouty** → „+ Playout” | rodzaj, kanał (tylko bez playoutu), harmonogram; potem „Przebuduj” |
| przerwy, wypełnienie | **Wypełniacze**, **Znaki wodne** | |
| grafika i studio na żywo | **Grafika kanału**, **Sloty na żywo** | wymaga VizRS CG Playout; ramka **Podgląd anteny VizRS** pokazuje grafikę na wybranym kanale, **Otwórz w Trio ↗** / **Trio ↗** na karcie kanału w Na antenie otwiera Trio na antenie tego kanału – przy pierwszym wejściu w tej przeglądarce (komunikat „wymagany token” albo „token wyjścia pozwala tylko na odczyt”) dopisz do adresu token operatora VizRS (`?token=…` albo `&token=…`), potem Trio go pamięta; w Grafice kanału tabela **„Kiedy strona wchodzi na antenę”** pokazuje przy każdej stronie czas z Trio, reguły CG z jej TAKE i element legacy (**Sprawdź przypisania w ramówce** szuka go w harmonogramach, blokach i deco); ostrzeżenie „może wejść dwa razy” znaczy, że stronę wprowadza i reguła, i ramówka – zostaw jedno; tabela reguł **„Kiedy / Co / Ile”** nad edytorem streszcza aktywne reguły (klik w wiersz przenosi do edycji); w **Edytorze reguł** przy każdym momencie (start pozycji, przed końcem audycji, przerwa reklamowa) wybierz „grafiki VizRS”, potem **+ Grafika**: akcja (Wprowadź, Dalej, Zdejmij, Podmień dane), strona z listy show i **+ pole** – nazwa pola strony i wartość: tekst albo symbol z listy **{…}** (tytuł, następny program, godzina startu); kroki idą po kolei (↑ ↓), w szarym wierszu widać komendę dla VizRS, **Tekst** pokazuje ją do ręcznej edycji; reguły audycji i kanałów to zwijane karty z polami „jak globalnie” / „wyłączone” / „grafiki VizRS”; w karcie kanału pole **Na przejęcie przez live ingest** pozwala ustawić logo na przejęcie (Grafika kanału → Reguły kanałów → karta kanału → „Na przejęcie przez live ingest” → wybierz stronę, np. 7510 – logo NA ŻYWO na BTB → **Zapisz reguły grafiki**; wymaga CG Playout z cechą `on_live`); w trakcie przejęcia kanału grafika ramówki znika i wraca sama po zejściu studia, a w Slotach na żywo karta kanału pokazuje wtedy wiersz **Grafika: live – ramówka wstrzymana**; na koniec **Zapisz reguły grafiki**; w Slotach na żywo tabela **Wejścia studia** podsumowuje stan i obraz wszystkich wejść RTMP/SRT, a **Ustaw wejścia…** dodaje, zmienia i usuwa wejścia kanałów (RTMP z kluczem albo SRT z hasłem, port z listy, „Losuj” daje nowy klucz; po zmianie podaj nadawcy nowe dane z „Jak nadawać na ten kanał”); **ZDEJMIJ** zdejmuje studio z anteny (z potwierdzeniem), **WPUŚĆ** wpuszcza |
| zmienić konfigurację systemu / legacy | **Ustawienia** | sekcje legacy (FFmpeg, playouty, XMLTV, skaner, logi, web GUI) oraz Panel i Most jako panele w siatce (zamiast zakładek); każdy ma własne „Zapisz” w nagłówku |
| przejrzeć logi i zdarzenia | **Logi** | wybór źródła na pasku: „ErsatzTV legacy” (z progiem poziomu i stronicowaniem) albo „VizRS CG Playout” (dziennik zdarzeń: przejęcia, ZDEJMIJ/WPUŚĆ, ramówka); filtr tekstem |
| sprawdzić stan stosu i kanałów | **Diagnostyka** | panel „Stan” na górze (tabela Stan / Składnik / Opis z pigułkami OK / UWAGA / BŁĄD / INFO dla legacy, mostu, kontroli, playoutów, CG Playout i VizRS; „Sprawdź ponownie”), kontrole legacy, wersje i sprzęt, test odtwarzania, konserwacja |

### Przebudowa ramówki

Zmiana harmonogramu, bloków, szablonów, kalendarza czy deco **nie zmienia od razu tego, co już zaplanowane**. Po zmianie:

- **Przebuduj** (Na antenie albo Playouty – także w wierszu listy, obok „Zbudowany do”) – dobudowuje i odświeża przyszłe pozycje; to wystarcza zwykle;
- **Od nowa** – kasuje zaplanowane pozycje i układa wszystko jeszcze raz (np. po dużej przebudowie ramówki; kanał „przeskoczy”);
- **Resetuj wszystkie** (Playouty) – „Od nowa” dla wszystkich kanałów, z potwierdzeniem.

Przebudowa idzie w tle legacy – po kilku sekundach odśwież Na antenie / EPG.

### Usuwanie

Każde usunięcie pyta o potwierdzenie i mówi, co jeszcze zniknie (np. usunięcie harmonogramu usuwa playouty kanałów, które go używają).

### Komunikaty

Czerwony komunikat „legacy odrzucił: …” to odmowa ErsatzTV legacy z jego tekstem po angielsku (np. „Channel number must be unique”) – popraw pole i zapisz jeszcze raz. Nic nie zostało zapisane. Długi wyjątek legacy panel skraca do pierwszego zdania; całość widać po najechaniu na komunikat.

## 3. Awarie – co zrobić

| Objaw | Co zrobić |
|---|---|
| pasek: most **czerwony**, ekrany puste | legacy nie działa albo się restartuje (start trwa do minuty). Odczekaj; dalej czerwony → **Diagnostyka** (panel „Stan” na górze z komunikatem błędu) i administrator (RUNBOOK „Awarie”) |
| licznik ostrzeżeń playoutów > 0 | **Playouty** – kanał z błędem budowy; zwykle pusta kolekcja albo brak treści w harmonogramie. Popraw, „Przebuduj” |
| kanał nie gra / czarny obraz | **Podgląd** kanału albo **Diagnostyka** → panel „Stan” i test odtwarzania („Odtwórz w panelu”, odnośnik HLS/TS do VLC); **Logi** (wybór źródła: legacy z progiem Warning/Error lub zdarzenia CG Playout); sprawdź, czy kanał ma playout i pozycje na **Na antenie** |
| brak nowych plików | **Biblioteki** → „Skanuj”; plik w katalogu, którego biblioteka nie obejmuje, nie pojawi się |
| EPG puste u widzów | „Przebuduj”; XMLTV pokazuje tylko zbudowane dni (Ustawienia → panel Przewodnik XMLTV) |
| „wymaga komendy X na białej liście mostu” | wersja mostu nie zna funkcji – administrator |
| „za dużo nieudanych logowań” | odczekaj minutę, wpisz token ponownie |
| studio nie wchodzi na antenę | **Sloty na żywo**: czy jest sygnał, czy kanał nie jest WSTRZYMANY (WPUŚĆ) |

Zawsze przy zgłoszeniu do administratora: godzina, kanał, treść komunikatu (albo zrzut ekranu).
