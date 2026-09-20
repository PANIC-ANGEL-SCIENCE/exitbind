#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
json=0; before=HEAD; after=WORKTREE
while test "$#" -gt 0; do
  case "$1" in
    --json) json=1;;
    --before) shift; before=${1:?missing before revision};;
    --after) shift; after=${1:?missing after revision};;
    -h|--help) printf '%s\n' 'usage: context-surface-inventory.sh [--json] [--before REV] [--after REV|WORKTREE]'; exit 0;;
    *) if test "$before" = HEAD; then before=$1; elif test "$after" = WORKTREE; then after=$1; else exit 2; fi;;
  esac
  shift
done
test "$before" = WORKTREE || git rev-parse --verify "$before^{commit}" >/dev/null 2>&1 || { printf '%s\n' 'before revision is not immutable' >&2; exit 2; }
test "$after" = WORKTREE || git rev-parse --verify "$after^{commit}" >/dev/null 2>&1 || { printf '%s\n' 'after revision is not immutable' >&2; exit 2; }
entries='AGENTS.md|bootstrap|root|none
skills/exitbind-bootstrap/SKILL.md|bootstrap|router|none
skills/exitbind/SKILL.md|activated-skill|router|skills/exitbind/references/preservation.md
skills/exitbind/references/preservation.md|delayed-reference|reference|none
plugins/exitbind/skills/exitbind/SKILL.md|packaged-duplicate|projection|skills/exitbind/SKILL.md
plugins/exitbind/skills/exitbind/references/preservation.md|packaged-duplicate|projection|skills/exitbind/references/preservation.md
.agents/skills/exitbind/SKILL.md|generated-host|host-copy|skills/exitbind/SKILL.md
.claude/skills/exitbind/SKILL.md|generated-host|host-copy|skills/exitbind/SKILL.md
exitbind/agents/lead.md|generated-host|role-packet|none
exitbind/agents/worker.md|generated-host|role-packet|none
exitbind/agents/reviewer.md|generated-host|role-packet|none'
tmp=$(mktemp -d "${TMPDIR:-/tmp}/exitbind-context-inventory.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
read_at() { if test "$1" = WORKTREE; then test -f "$2" && cp "$2" "$3" || return 1; else git cat-file blob "$1:$2" >"$3" 2>/dev/null || return 1; fi; }
measure() {
  ref=$1; path=$2; file=$3
  if read_at "$ref" "$path" "$file"; then
    printf '%s|%s|%s' "$(wc -c <"$file" | tr -d ' ')" "$(wc -l <"$file" | tr -d ' ')" "$(sha256sum "$file" | awk '{print $1}')"
  else printf 'null|null|null'; fi
}
surface_json() {
  path=$1; stage=$2; projection=$3; duplicate=$4; key=$5
  old=$(measure "$before" "$path" "$tmp/$key.old"); new=$(measure "$after" "$path" "$tmp/$key.new")
  ob=${old%%|*}; or=${old#*|}; ol=${or%%|*}; oh=${or#*|}
  nb=${new%%|*}; nr=${new#*|}; nl=${nr%%|*}; nh=${nr#*|}
  test "$oh" = null && ohj=null || ohj="\"$oh\""
  test "$nh" = null && nhj=null || nhj="\"$nh\""
  test "$duplicate" = none && dup=null || dup="\"$duplicate\""
  printf '{"path":"%s","stage":"%s","projection":"%s","expansionReference":"%s","before":{"bytes":%s,"lines":%s,"sha256":%s},"after":{"bytes":%s,"lines":%s,"sha256":%s},"duplicateOf":%s}' "$path" "$stage" "$projection" "$duplicate" "$ob" "$ol" "$ohj" "$nb" "$nl" "$nhj" "$dup"
}

if test "$json" = 1; then
  test "$before" != WORKTREE || { printf '%s\n' 'before revision must be immutable' >&2; exit 2; }
  printf '{"method":"wc -c/wc -l plus immutable revision hashes and deterministic representative fixtures","beforeRevision":"%s","afterRevision":"%s","surfaces":[' "$before" "$after"
  first=1
  printf '%s\n' "$entries" | while IFS='|' read -r path stage projection duplicate; do
    test -n "$path" || continue
    test "$first" = 1 || printf ','
    first=0
    key=$(printf '%s' "$path" | sha256sum | cut -c1-12)
    surface_json "$path" "$stage" "$projection" "$duplicate" "$key"
  done
  product_source="$tmp/product-source"
  product_target="$tmp/product-target"
  if test "$after" = WORKTREE; then
    product_source="$root"
    source_mode=worktree
    source_revision=$(git rev-parse HEAD)
    tracked_diff_sha256=$(git diff --binary -- . | sha256sum | awk '{print $1}')
  else
    source_mode=immutable
    source_revision=$(git rev-parse "$after^{commit}")
    tracked_diff_sha256=null
    mkdir -p "$product_source"
    git archive --format=tar "$source_revision" >"$tmp/product-source.tar"
    tar -xf "$tmp/product-source.tar" -C "$product_source"
  fi
  (cd "$product_source" && CARGO_TARGET_DIR="$product_target" CARGO_INCREMENTAL=0 RUSTFLAGS="--remap-path-prefix=$product_source=/exitbind-source" cargo build --locked --quiet --bin exitbind)
  binary="$product_target/debug/exitbind"
  test -x "$binary" || { printf '%s\n' 'product binary build did not produce an executable' >&2; exit 2; }
  binary_sha256=$(sha256sum "$binary" | awk '{print $1}')
  python3 -c 'import json,sys; print(json.dumps({"mode":sys.argv[1],"revision":sys.argv[2],"trackedDiffSha256":None if sys.argv[3]=="null" else sys.argv[3]},separators=(",",":")))' "$source_mode" "$source_revision" "$tracked_diff_sha256" >"$tmp/source.json"
  printf '%s\n' "$binary_sha256" >"$tmp/binary.sha256"
  fixture="$tmp/product-fixture"
  mkdir -p "$fixture"
  (cd "$fixture" && "$binary" init --mode portable --root . >/dev/null)
  run_product() { (cd "$fixture" && "$binary" "$@" --config exitbind.json); }
  json_field() { python3 -c 'import json,sys; v=json.load(open(sys.argv[1])); [v:=v[p] for p in sys.argv[2].split(".")]; print(v)' "$1" "$2"; }
  packet_metric() { python3 -c 'import json,sys; p=json.load(open(sys.argv[1]))["next"]["packet"]; print(len(json.dumps(p,separators=(",",":"),sort_keys=True).encode()),len(p["context"]["expansions"]))' "$1"; }
  start="$tmp/start.json"
  run_product work begin change --goal inventory-fixture --check-command true >"$start"
  work=$(json_field "$start" work)
  lead_metric=$(packet_metric "$start")
  lead_assignment=$(json_field "$start" next.assignment)
  printf 'scope\n' | run_product work return "$work" "$lead_assignment" --outcome scoped >/dev/null
  : >"$tmp/history.jsonl"
  for attempt in 1 2 3; do
    worker="$tmp/worker-$attempt.json"
    run_product work next "$work" >"$worker"
    if test "$attempt" = 1; then worker_metric=$(packet_metric "$worker"); fi
    ledger_file=$(find "$fixture" -type f -path '*/runs/*.jsonl' -print | sort | head -n 1)
    test -n "$ledger_file" || { printf '%s\n' 'product fixture ledger unavailable' >&2; exit 2; }
    history_events=$(wc -l <"$ledger_file" | tr -d ' ')
    python3 -c 'import json,sys; p=json.load(open(sys.argv[1]))["next"]["packet"]; c=p["context"]; print(json.dumps({"historyEvents":int(sys.argv[2]),"routineExpansionCount":len(c["expansions"]),"routineFieldsBounded":len(c["evidence"])<=8 and len(p["upstreamArtifacts"])<=8},separators=(",",":")))' "$worker" "$history_events" >>"$tmp/history.jsonl"
    worker_assignment=$(json_field "$worker" next.assignment)
    printf 'worker\n' | run_product work return "$work" "$worker_assignment" --outcome completed >/dev/null
    run_product work check "$work" >/dev/null
    reviewer="$tmp/reviewer-$attempt.json"
    run_product work next "$work" >"$reviewer"
    if test "$attempt" = 1; then reviewer_metric=$(packet_metric "$reviewer"); fi
    reviewer_assignment=$(json_field "$reviewer" next.assignment)
    printf 'review\n' | run_product work return "$work" "$reviewer_assignment" --outcome approved >/dev/null
    lead="$tmp/lead-$attempt.json"
    run_product work next "$work" >"$lead"
    if test "$attempt" -lt 3; then
      lead_assignment=$(json_field "$lead" next.assignment)
      printf 'repair-request\n' | run_product work return "$work" "$lead_assignment" --outcome rework >/dev/null
    fi
  done
  printf '%s\n' "$lead_metric" >"$tmp/lead.metric"
  printf '%s\n' "$worker_metric" >"$tmp/worker.metric"
  printf '%s\n' "$reviewer_metric" >"$tmp/reviewer.metric"
  dynamic=$(python3 -c 'import json,pathlib,sys; d=pathlib.Path(sys.argv[1]); m=lambda s: dict(zip(("bytes","expansionCount"),map(int,s.split()))); source=json.loads((d/"source.json").read_text()); binary=(d/"binary.sha256").read_text().strip(); print(json.dumps({"method":"product CLI generated packets and ledger fixtures","sourceIdentity":source,"sourceMode":source["mode"],"sourceRevision":source["revision"],"trackedDiffSha256":source["trackedDiffSha256"],"binarySha256":binary,"packets":{"lead":m((d/"lead.metric").read_text()),"worker":m((d/"worker.metric").read_text()),"reviewer":m((d/"reviewer.metric").read_text())},"historyGrowth":[json.loads(x) for x in (d/"history.jsonl").read_text().splitlines()],"tokenTelemetry":None,"humanTelemetry":None},separators=(",",":")))' "$tmp")
  printf '],"dynamic":%s,"telemetry":"exact-local-files-and-product-fixtures"}\n' "$dynamic"
  exit 0
fi

printf '%-52s %10s %8s  %s\n' surface bytes lines stage
printf '%-52s %10s %8s  %s\n' '----------------------------------------------------' '----------' '--------' '-----'
printf '%s\n' "$entries" | while IFS='|' read -r path stage projection duplicate; do
  test -n "$path" || continue
  if test -f "$path"; then
    printf '%-52s %10s %8s  %s\n' "$path" "$(wc -c <"$path" | tr -d ' ')" "$(wc -l <"$path" | tr -d ' ')" "$stage"
  else
    printf '%-52s %10s %8s  %s\n' "$path" 0 0 "$stage"
  fi
done
total=0
count=0
for path in AGENTS.md skills/exitbind-bootstrap/SKILL.md skills/exitbind/SKILL.md skills/exitbind/references/preservation.md plugins/exitbind/skills/exitbind/SKILL.md plugins/exitbind/skills/exitbind/references/preservation.md .agents/skills/exitbind/SKILL.md .claude/skills/exitbind/SKILL.md exitbind/agents/lead.md exitbind/agents/worker.md exitbind/agents/reviewer.md; do
  if test -f "$path"; then
    total=$((total + $(wc -c <"$path" | tr -d ' ')))
    count=$((count + 1))
  fi
done
printf '\nTotal instruction surface: %s bytes across %s files\n' "$total" "$count"
