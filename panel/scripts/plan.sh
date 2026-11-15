#!/usr/bin/env bash
# Automat planu (PLAN_FILE, domyślnie docs/plany/PLAN-mcr.md – jedyny plan rozwoju; punkty K1, Q1… – litera i numer):
# jeden punkt = jedna świeża sesja Claude Code bez operatora.
#
#   scripts/plan.sh status                 # tabela planu i następny punkt
#   scripts/plan.sh next                   # tylko identyfikator następnego punktu (pusty, gdy nie ma)
#   scripts/plan.sh run [-n N] [-p Kn] [--dry-run]
#        -n N       ile punktów po kolei (domyślnie 1); zatrzymuje się na pierwszym niepowodzeniu
#        -p Kn      ten punkt zamiast następnego z tabeli
#        --dry-run  pokaż punkt i polecenie sesji, nic nie uruchamiaj
#
# Każdy punkt: czyste drzewo git → `claude -p` w nowej sesji (realizacja wg CLAUDE.md, lokalny commit, bez push)
# → niezależna kontrola: Stan w tabeli = „zrobione”, nowy commit, czyste drzewo, PLAN_CHECK zielony
#   (domyślnie tests/deploy.sh – szybkie testy, wdrożenie panelu na lokalny stos, przejście tylko do odczytu;
#   PLAN_CHECK=tests/gate.sh – pełna bramka ze stosem testowym i e2e, jak w CI).
# Dziennik sesji: .plan-logs/<punkt>-<czas>.log
#
# Automat pomija punkty, których nie zrobi sam: Stan inny niż „do zrobienia” (np. „ręcznie (z użytkownikiem)”)
# i punkty ze znacznikiem „CG” w zakresie (zmiana API VizRS CG Playout = drugie repozytorium, ten sam cykl –
# robi się je w sesji z operatorem). Commit trafia na bieżącą gałąź (realizacja PLAN-mcr: next-v).
#
# Zmienne: PLAN_PERMISSION_MODE (domyślnie auto), PLAN_MODEL, PLAN_BUDGET_USD (limit kosztu sesji),
#          PLAN_TIMEOUT (np. 4h), PLAN_CLAUDE_ARGS (dodatkowe argumenty claude).
set -euo pipefail
cd "$(dirname "$0")/.."
PLAN=${PLAN_FILE:-docs/plany/PLAN-mcr.md}
LOGS=.plan-logs
CHECK=${PLAN_CHECK:-tests/deploy.sh}

# Wiersze tabeli: punkt<TAB>wymaga(rozwinięte, spacjami; „-” = brak)<TAB>stan<TAB>zakres
table() {
  python3 -I - "$PLAN" <<'PY'
import re, sys
rows = []
for line in open(sys.argv[1], encoding='utf-8'):
    m = re.match(r'^\|\s*([A-Z]\d+)\s*\|(.*?)\|(.*?)\|(.*?)\|\s*$', line)
    if not m:
        continue
    pid, scope, req, state = (x.strip() for x in m.groups())
    deps = []
    for part in re.split(r'[,\s]+', req):
        r = re.match(r'^([A-Z])(\d+)[–-]\1(\d+)$', part)
        if r:
            deps += [f'{r[1]}{i}' for i in range(int(r[2]), int(r[3]) + 1)]
        elif re.match(r'^[A-Z]\d+$', part):
            deps.append(part)
    # „-” przy braku wymagań: read z IFS=tab skleja puste pola (tab to biały znak IFS)
    print(f"{pid}\t{' '.join(deps) or '-'}\t{state}\t{scope}")
PY
}

done_points() { table | awk -F'\t' '$3 ~ /^zrobione/ {print $1}'; }

next_point() {
  local done; done=" $(done_points | tr '\n' ' ') "
  table | while IFS=$'\t' read -r id deps state _; do
    [[ "$state" == do\ zrobienia* ]] || continue
    needs_cg "$id" && continue
    local ok=1
    for d in $deps; do [ "$d" = - ] || [[ "$done" == *" $d "* ]] || ok=0; done
    if [ $ok = 1 ]; then echo "$id"; break; fi
  done
}

state_of() { table | awk -F'\t' -v p="$1" '$1 == p {print $3}'; }

# Punkt zmienia API VizRS CG Playout (znacznik „CG” w zakresie) – automat go nie robi
needs_cg() { table | awk -F'\t' -v p="$1" '$1 == p {print $4}' | grep -qE '(^|[^[:alnum:]])CG([^[:alnum:]]|$)'; }

prompt_for() {
  cat <<P
Zrealizuj punkt $1 planu $PLAN – cały i tylko ten punkt (gałąź: $(git branch --show-current)).

Działasz bez operatora (automat scripts/plan.sh). Zasady tej sesji:
- Postępuj wg CLAUDE.md i zasad „Jak pracować” z planu: przeczytaj CLAUDE.md, docs/STATUS.md, plan $PLAN (zasady pracy, tabelę i opis punktu $1), docs/LUKI.md (gdy punkt zamyka lukę – usuń jej wiersz), docs/DOKUMENTACJA.md, git log -5. Testy: $CHECK (tests/deploy.sh = szybkie testy + wdrożenie panelu na lokalny stos + przejście po ekranach tylko do odczytu; stosu testowego nie stawiaj, chyba że to tests/gate.sh).
- Nie zadawaj pytań – nikt nie odpowie. Decyzje „do sprawdzenia” podejmij sam, zachowawczo, i zapisz je z uzasadnieniem w docs/DOKUMENTACJA.md (i krótko pod punktem w planie).
- Legacy bez zmian, C# tylko w moście, serwer bez zależności, UI bez bibliotek i bez inline (CSP), lokalny stos (virtrs-*) zmieniaj tylko przez tests/deploy.sh (podmienia sam panel) – bez zapisów do legacy i CG Playout, bez restartu innych usług.
- Nie zmieniaj innych repozytoriów, nie publikuj niczego (bez git push, bez wypychania obrazów, bez zakładania issue/PR). Czego punkt wymaga poza tym repozytorium, opisz w docs/DOKUMENTACJA.md jako krok ręczny.
- Gotowe = warunki „gotowe” punktu, nowe przypadki e2e w tests/e2e.cjs (uruchamia je CI), zrzuty nowych widoków obejrzane (DEPLOY_SHOTS=<katalog poza repo> tests/deploy.sh), docs/DOKUMENTACJA.md, CHANGELOG.md (Niewydane), w tabeli planu Stan punktu $1 = „zrobione (RRRR-MM-DD)”, a $CHECK zielony.
- Na koniec zrób lokalny commit (ten automat to prośba użytkownika o commit): pliki dodawaj jawnie, wiadomość zaczynaj od „$1:”. Po commicie drzewo ma być czyste. Bez push.
- Jeśli punktu nie da się dokończyć: przywróć działający stan ($CHECK zielony), w tabeli ustaw Stan = „przerwane: <krótki powód>”, opisz szczegóły pod punktem w planie, zrób commit „$1: przerwane – …” i zakończ.
- Nie zaczynaj kolejnego punktu.
P
}

run_point() {
  local p="$1" dry="$2"
  local st; st=$(state_of "$p")
  [ -n "$st" ] || { echo "nie ma punktu $p w $PLAN" >&2; return 1; }
  [[ "$st" == zrobione* ]] && { echo "$p już zrobiony ($st)" >&2; return 1; }
  [[ "$st" == do\ zrobienia* || "$st" == przerwane* ]] || { echo "$p ma Stan „$st” – automat go nie robi" >&2; return 1; }
  needs_cg "$p" && { echo "$p zmienia API VizRS CG Playout (znacznik CG) – zrób go w sesji z operatorem, razem z repo CG Playout" >&2; return 1; }
  local args=(-p "$(prompt_for "$p")" --permission-mode "${PLAN_PERMISSION_MODE:-auto}" --output-format text)
  [ -n "${PLAN_MODEL:-}" ] && args+=(--model "$PLAN_MODEL")
  [ -n "${PLAN_BUDGET_USD:-}" ] && args+=(--max-budget-usd "$PLAN_BUDGET_USD")
  # shellcheck disable=SC2206
  [ -n "${PLAN_CLAUDE_ARGS:-}" ] && args+=($PLAN_CLAUDE_ARGS)
  if [ "$dry" = 1 ]; then
    echo "== $p (próba): claude ${args[*]:2}"
    prompt_for "$p"
    return 0
  fi
  if [ -n "$(git status --porcelain)" ]; then
    echo "drzewo git nie jest czyste – zrób commit albo odłóż zmiany przed $p:" >&2
    git status --short >&2
    return 1
  fi
  mkdir -p "$LOGS"
  local log; log="$LOGS/$p-$(date +%Y%m%d-%H%M%S).log"
  local before; before=$(git rev-parse HEAD)
  echo "== $p: sesja Claude Code (dziennik: $log)"
  local t=()
  [ -n "${PLAN_TIMEOUT:-}" ] && t=(timeout "$PLAN_TIMEOUT")
  if ! "${t[@]}" claude "${args[@]}" </dev/null >"$log" 2>&1; then
    echo "$p: sesja zakończona błędem – zobacz $log" >&2
    return 1
  fi
  tail -n 25 "$log"

  echo "== $p: kontrola"
  st=$(state_of "$p")
  local fail=0
  [ "$(git rev-parse HEAD)" != "$before" ] || { echo "BŁĄD brak nowego commita" >&2; fail=1; }
  [ -z "$(git status --porcelain)" ] || { echo "BŁĄD niezacommitowane zmiany po sesji" >&2; git status --short >&2; fail=1; }
  if [[ "$st" == przerwane* ]]; then
    echo "$p przerwany przez sesję: $st" >&2
    return 1
  fi
  [[ "$st" == zrobione* ]] || { echo "BŁĄD Stan $p = „$st”, a powinien być „zrobione”" >&2; fail=1; }
  if ! $CHECK >>"$log" 2>&1; then echo "BŁĄD $CHECK czerwony – $log" >&2; fail=1; fi
  [ $fail = 0 ] || return 1
  echo "$p: zrobiony i sprawdzony ($(git log -1 --format='%h %s'))"
}

case "${1:-status}" in
  status)
    echo "plan: $PLAN, gałąź: $(git branch --show-current)"
    table | awk -F'\t' '{printf "%-4s %-28s %s\n", $1, $3, $4}'
    n=$(next_point); echo; echo "następny: ${n:-brak (wszystko zrobione albo zablokowane)}"
    cg=$(table | awk -F'\t' '$3 ~ /^do zrobienia/ {print $1}' | while read -r i; do if needs_cg "$i"; then printf '%s ' "$i"; fi; done)
    [ -z "$cg" ] || echo "z operatorem (API CG Playout): $cg"
    ;;
  next) next_point ;;
  run)
    shift
    count=1 point="" dry=0
    while [ $# -gt 0 ]; do
      case "$1" in
        -n) count="$2"; shift 2 ;;
        -p) point="$2"; shift 2 ;;
        --dry-run) dry=1; shift ;;
        *) echo "nieznana opcja: $1" >&2; exit 2 ;;
      esac
    done
    command -v claude >/dev/null || { echo "brak polecenia claude w PATH" >&2; exit 2; }
    for ((i = 0; i < count; i++)); do
      p="${point:-$(next_point)}"
      [ -n "$p" ] || { echo "nic do zrobienia: brak punktu „do zrobienia” ze spełnionym „Wymaga”"; exit 0; }
      run_point "$p" "$dry" || exit 1
      point=""
      [ "$dry" = 1 ] && break
    done
    ;;
  *) sed -n '2,20p' "$0"; exit 2 ;;
esac
