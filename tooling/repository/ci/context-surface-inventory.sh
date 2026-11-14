#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
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
entries='AGENTS.md|source-reading|explicit|none
CONTRIBUTING.md|contributor-only|explicit|none
skills/exitbind-bootstrap/SKILL.md|bootstrap|router|none
skills/exitbind/SKILL.md|activated-skill|router|skills/exitbind/references/preservation.md
skills/exitbind/references/preservation.md|delayed-reference|reference|none
skills/exitbind/references/operators.md|delayed-reference|reference|none
plugins/exitbind/skills/exitbind/SKILL.md|packaged-duplicate|projection|skills/exitbind/SKILL.md
plugins/exitbind/skills/exitbind/references/preservation.md|packaged-duplicate|projection|skills/exitbind/references/preservation.md
plugins/exitbind/skills/exitbind/references/operators.md|packaged-duplicate|projection|skills/exitbind/references/operators.md
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

# BEGIN inventory receipt validators
validate_inventory_submit() {
  python3 - "$@" <<'PY'
import json,re,sys
try:
    path,work,assignment,outcome=sys.argv[1:]
    value=json.load(open(path)); event=value['event']; event_hash=event['eventSha256']
    if (value['work']!=work or value['assignment']!=assignment or value['status']!='recorded'
        or value['effect']!='recorded' or value['outcome']!=outcome
        or event['action']!='submit' or event['outcome']!=outcome
        or value['eventSha256']!=event_hash
        or not isinstance(event_hash,str) or not re.fullmatch(r'[0-9a-f]{64}',event_hash)):
        raise ValueError('submission is not the current recorded result')
    print(event_hash)
except (OSError,ValueError,KeyError,TypeError) as error:
    sys.exit('inventory submission refused: '+str(error))
PY
}

validate_inventory_check() {
  python3 - "$@" <<'PY'
import json,re,sys
try:
    path,work,target=sys.argv[1:]
    value=json.load(open(path)); event=value['event']; event_hash=event['eventSha256']
    if (value['work']!=work or value['status']!='recorded' or value['effect']!='recorded'
        or event['action']!='check' or event['targetEventSha256']!=target
        or type(event['result']['code']) is not int or event['result']!={'kind':'exit','code':0}
        or value['result']!=event['result']
        or value['eventSha256']!=event_hash or value['targetEventSha256']!=target
        or not isinstance(event_hash,str) or not re.fullmatch(r'[0-9a-f]{64}',event_hash)):
        raise ValueError('check is not the recorded success for the current result')
    print(event_hash)
except (OSError,ValueError,KeyError,TypeError) as error:
    sys.exit('inventory check refused: '+str(error))
PY
}

validate_inventory_evidence_route() {
  python3 - "$@" <<'PY'
import json,sys
from pathlib import Path
try:
    detail,binary,fixture,work=sys.argv[1:]; d=json.load(open(detail))
    forms=d['actionForms']; recipient=d['recipient']; assignment=recipient['assignment']
    choices=[c for c in forms['choices'] if c.get('label')=='evidence']
    if (d['work']!=work or d['complete'] is not True or d['valid'] is not True
        or recipient['role']!='worker' or forms['work']!=work or forms['assignment']!=assignment
        or forms['binding']!=d['binding'] or forms['choiceActor']['role']!='lead'
        or forms['recovery']['state']!='replan_required' or forms['recovery'].get('heldResults')
        or len(choices)!=1 or sorted(choices[0]['placeholders'])!=['ARTIFACT_PATH','ARTIFACT_ROOT']
        or not isinstance(choices[0]['inputConstraints']['ARTIFACT_ROOT']['allowedValues'],list)
        or 'state' not in choices[0]['inputConstraints']['ARTIFACT_ROOT']['allowedValues']):
        raise ValueError('current Lead evidence form is unavailable')
    command=choices[0]['command']
    argv=command['argv']
    if not isinstance(argv,list) or len(argv)!=11: raise ValueError('invalid evidence argv')
    expected=[argv[0],'work','evidence',work,assignment,'--artifact-root','<ARTIFACT_ROOT>',
              '--artifact','<ARTIFACT_PATH>','--config',argv[-1]]
    if (command['sameConfigRequired'] is not False or command['sameExecutableRequired'] is not False
        or argv!=expected or not Path(argv[0]).is_absolute() or Path(argv[0]).resolve()!=Path(binary).resolve()
        or not Path(argv[-1]).is_absolute() or Path(argv[-1]).resolve()!=(Path(fixture)/'exitbind.json').resolve()):
        raise ValueError('current Lead evidence command is incomplete or foreign')
except (OSError,ValueError,KeyError,TypeError) as error:
    sys.exit('inventory evidence route refused: '+str(error))
PY
}

inventory_history_evidence() {
  python3 - "$@" <<'PY'
import json,re,sys
from pathlib import Path
try:
    (work,detail_path,history_path,next1,next2,ledger1,ledger2,
     submit1,submit2,check1,check2)=sys.argv[1:]
    d=json.load(open(detail_path)); forms=d['actionForms']; recipient=d['recipient']
    loop=d['sections']['assignment']['assignment']['context']['loop']
    choices=[c for c in forms['choices'] if c.get('label')=='evidence']
    if (d['work']!=work or d['complete'] is not True or d['valid'] is not True
        or recipient['role']!='worker' or forms['work']!=work
        or forms['assignment']!=recipient['assignment'] or forms['binding']!=d['binding']
        or forms['choiceActor']['role']!='lead' or forms['recovery']['state']!='replan_required'
        or forms['recovery'].get('heldResults') or len(choices)!=1
        or sorted(choices[0]['placeholders'])!=['ARTIFACT_PATH','ARTIFACT_ROOT']
        or loop['state']!='replan_required' or type(loop['spent']) is not int or loop['spent']!=2
        or type(loop['noInformationStreak']) is not int or loop['noInformationStreak']!=2
        or loop['evidence']!=[] or loop['seenEvidenceSha256']!=[]):
        raise ValueError('current recovery or prior evidence does not match the fixture')
    samples=[json.loads(line) for line in Path(history_path).read_text().splitlines()]
    if len(samples)!=2:
        raise ValueError('two observed history samples are required')
    checks=[]
    for sample,npath,lpath,spath,cpath in zip(samples,(next1,next2),(ledger1,ledger2),
                                             (submit1,submit2),(check1,check2)):
        n=json.load(open(npath)); p=n['next']['packet']; context=p['context']
        s=json.load(open(spath)); c=json.load(open(cpath)); se=s['event']; ce=c['event']
        sh=se['eventSha256']; ch=ce['eventSha256']; count=Path(lpath).read_bytes().count(b'\n')
        bounded=len(context['evidence'])<=8 and len(p['upstreamArtifacts'])<=8
        if (n['work']!=work or n['next']['role']!='worker' or s['work']!=work
            or s['assignment']!=n['next']['assignment'] or s['status']!='recorded'
            or s['effect']!='recorded' or s['outcome']!='completed'
            or se['action']!='submit' or se['outcome']!='completed' or s['eventSha256']!=sh
            or c['work']!=work or c['status']!='recorded' or c['effect']!='recorded'
            or ce['action']!='check' or ce['targetEventSha256']!=sh
            or type(ce['result']['code']) is not int or ce['result']!={'kind':'exit','code':0}
            or c['result']!=ce['result']
            or c['eventSha256']!=ch or c['targetEventSha256']!=sh
            or not isinstance(sh,str) or not re.fullmatch(r'[0-9a-f]{64}',sh)
            or not isinstance(ch,str) or not re.fullmatch(r'[0-9a-f]{64}',ch)
            or type(sample['historyEvents']) is not int or sample['historyEvents']!=count
            or count<=0 or type(sample['routineExpansionCount']) is not int
            or sample['routineExpansionCount']!=len(context['expansions'])
            or sample['routineFieldsBounded'] is not True or not bounded):
            raise ValueError('history sample or checked Worker result is not the observed fact')
        checks.append({'eventSha256':ch,'targetEventSha256':sh,'result':ce['result']})
    if (samples[0]['historyEvents']>=samples[1]['historyEvents']
        or checks[0]['eventSha256']==checks[1]['eventSha256']
        or checks[0]['targetEventSha256']==checks[1]['targetEventSha256']):
        raise ValueError('history observations are duplicate or uninformative')
    print(json.dumps({'purpose':'Observed packet/history growth and checked prior Worker results for this fixture',
                      'work':work,'samples':samples,'checks':checks},sort_keys=True,separators=(',',':')))
except (OSError,ValueError,KeyError,TypeError) as error:
    sys.exit('inventory evidence refused: '+str(error))
PY
}
# END inventory receipt validators

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
  current_return() {
    run_product work detail "$work" --json >"$6"
    python3 - "$binary" "$fixture" "$work" "$1" "$2" "$3" "$4" "$5" "$6" <<'PY'
import json,subprocess,sys
from pathlib import Path
try:
    binary,fixture,work,role,assignment,outcome,reason,artifact,detail=sys.argv[1:]
    d=json.load(open(detail)); forms=d['actionForms']; recipient=d['recipient']
    choices=[c for c in forms['choices'] if c.get('label')==outcome]
    if (d['work']!=work or d['complete'] is not True or d['valid'] is not True
        or recipient['role']!=role or recipient['assignment']!=assignment
        or forms['work']!=work or forms['assignment']!=assignment or forms['binding']!=d['binding']
        or len(choices)!=1 or choices[0]['placeholders']!=['REASON']
        or choices[0]['stdin']['required'] is not True):
        raise ValueError('current return form does not match the selected actor')
    command=choices[0]['command']; argv=command['argv']
    if not isinstance(argv,list) or len(argv)!=13: raise ValueError('invalid return argv')
    expected=[argv[0],'work','return',work,assignment,'--current-binding',d['binding'],
              '--outcome',outcome,'--reason','<REASON>','--config',argv[-1]]
    if (command['sameConfigRequired'] is not False or command['sameExecutableRequired'] is not False
        or argv!=expected or not Path(argv[0]).is_absolute() or Path(argv[0]).resolve()!=Path(binary).resolve()
        or not Path(argv[-1]).is_absolute() or Path(argv[-1]).resolve()!=(Path(fixture)/'exitbind.json').resolve()):
        raise ValueError('current return command is incomplete or foreign')
    argv=[reason if arg=='<REASON>' else arg for arg in argv]
    result=subprocess.run(argv,cwd=fixture,input=Path(artifact).read_bytes())
    sys.exit(result.returncode)
except (OSError,ValueError,KeyError,TypeError) as error:
    sys.exit('inventory current return refused: '+str(error))
PY
  }
  current_check() {
    python3 - "$binary" "$fixture" "$work" "$1" <<'PY'
import json,subprocess,sys
from pathlib import Path
try:
    binary,fixture,work,path=sys.argv[1:]; value=json.load(open(path)); action=value['effectiveAction']
    argv=action['command']['argv']
    if not isinstance(argv,list) or len(argv)!=6: raise ValueError('invalid check argv')
    if (value['work']!=work or action['kind']!='check' or argv!=[argv[0],'work','check',work,'--config',argv[-1]]
        or not Path(argv[0]).is_absolute() or Path(argv[0]).resolve()!=Path(binary).resolve()
        or not Path(argv[-1]).is_absolute() or Path(argv[-1]).resolve()!=(Path(fixture)/'exitbind.json').resolve()):
        raise ValueError('current check command is incomplete or foreign')
    sys.exit(subprocess.run(argv,cwd=fixture).returncode)
except (OSError,ValueError,KeyError,TypeError) as error:
    sys.exit('inventory current check refused: '+str(error))
PY
  }
  current_history_evidence() {
    python3 - "$binary" "$fixture" "$work" "$1" "$2" <<'PY'
import hashlib,json,re,subprocess,sys
from pathlib import Path
try:
    binary,fixture,work,detail,artifact=sys.argv[1:]; d=json.load(open(detail)); forms=d['actionForms']
    choices=[c for c in forms['choices'] if c.get('label')=='evidence']
    if len(choices)!=1: raise ValueError('current Lead evidence form is unavailable')
    choice=choices[0]; command=choice['command']; assignment=d['recipient']['assignment']
    relative='.exitbind/artifacts/inventory-history-observed.json'
    argv=command['argv']
    if not isinstance(argv,list) or len(argv)!=11: raise ValueError('invalid evidence argv')
    expected=[argv[0],'work','evidence',work,assignment,'--artifact-root','<ARTIFACT_ROOT>',
              '--artifact','<ARTIFACT_PATH>','--config',argv[-1]]
    if (command['sameConfigRequired'] is not False or command['sameExecutableRequired'] is not False
        or argv!=expected or not Path(argv[0]).is_absolute() or Path(argv[0]).resolve()!=Path(binary).resolve()
        or not Path(argv[-1]).is_absolute() or Path(argv[-1]).resolve()!=(Path(fixture)/'exitbind.json').resolve()):
        raise ValueError('current Lead evidence command is incomplete or foreign')
    source=Path(artifact).read_bytes(); target=Path(fixture)/relative
    with target.open('xb') as output: output.write(source)
    argv=[{'<ARTIFACT_ROOT>':'state','<ARTIFACT_PATH>':relative}.get(arg,arg) for arg in command['argv']]
    result=subprocess.run(argv,cwd=fixture,stdout=subprocess.PIPE)
    sys.stdout.buffer.write(result.stdout); sys.stdout.buffer.flush()
    if result.returncode: sys.exit(result.returncode)
    value=json.loads(result.stdout); event=value['event']; governor=value['governor']
    evidence=event['governorEvent']['evidence']; digest=hashlib.sha256(source).hexdigest()
    loop=d['sections']['assignment']['assignment']['context']['loop']
    subject=d['sections']['assignment']['assignment']['context']['subject']['sha256']
    inputs=d['sections']['assignment']['residual']['snapshot']['inputsSha256']
    if (value['work']!=work or event['action']!='govern' or event['operation']!='evidence'
        or event['governorEvent']['action']!='evidence' or evidence['sha256']!=digest
        or not isinstance(event['eventSha256'],str) or not re.fullmatch(r'[0-9a-f]{64}',event['eventSha256'])
        or not isinstance(event['governorEvent']['eventSha256'],str)
        or not re.fullmatch(r'[0-9a-f]{64}',event['governorEvent']['eventSha256'])
        or evidence['bytes']!=len(source) or evidence['subjectSha256']!=subject
        or evidence['inputSha256']!=inputs or event['subjectSha256']!=subject or event['inputsSha256']!=inputs
        or governor['subjectSha256']!=subject or governor['state']!='ready'
        or type(governor['budget']) is not int or governor['budget']!=loop['budget']
        or type(governor['spent']) is not int or governor['spent']!=loop['spent']
        or type(governor['noInformationStreak']) is not int or governor['noInformationStreak']!=0
        or governor['seenEvidenceSha256']!=[digest]
        or len(governor['evidence'])!=1 or governor['evidence'][0]['sha256']!=digest
        or value['next']['assignment']!=assignment or value['next']['role']!='worker'):
        raise ValueError('evidence was not recorded for the same current Worker with spent authority preserved')
except (OSError,ValueError,KeyError,TypeError) as error:
    sys.exit('inventory current evidence refused: '+str(error))
PY
  }
  start="$tmp/start.json"
  run_product work begin change --goal 'Measure bounded packet and history growth across three Worker/check/Reviewer cycles' --check-command true >"$start"
  work=$(json_field "$start" work)
  lead_metric=$(packet_metric "$start")
  lead_assignment=$(json_field "$start" next.assignment)
  printf '%s\n' 'Measure packet and history growth over three Worker/check/Reviewer Work cycles.' >"$tmp/scope-artifact"
  current_return lead "$lead_assignment" scoped 'Measure three packet and history snapshots' "$tmp/scope-artifact" "$tmp/scope-detail.json" >"$tmp/scope-return.json"
  validate_inventory_submit "$tmp/scope-return.json" "$work" "$lead_assignment" scoped >/dev/null
  : >"$tmp/history.jsonl"
  for attempt in 1 2 3; do
    if test "$attempt" = 3; then
      run_product work detail "$work" --json >"$tmp/recovery-detail.json"
      validate_inventory_evidence_route "$tmp/recovery-detail.json" "$binary" "$fixture" "$work"
      inventory_history_evidence "$work" "$tmp/recovery-detail.json" "$tmp/history.jsonl" \
        "$tmp/worker-1.json" "$tmp/worker-2.json" "$tmp/ledger-1.jsonl" "$tmp/ledger-2.jsonl" \
        "$tmp/worker-return-1.json" "$tmp/worker-return-2.json" "$tmp/check-1.json" "$tmp/check-2.json" >"$tmp/history-observation.json"
      current_history_evidence "$tmp/recovery-detail.json" "$tmp/history-observation.json" >"$tmp/evidence-result.json"
      run_product work detail "$work" --json >"$tmp/recovered-detail.json"
      python3 - "$tmp/recovery-detail.json" "$tmp/recovered-detail.json" "$work" <<'PY'
import json,sys
try:
    before=json.load(open(sys.argv[1])); after=json.load(open(sys.argv[2])); work=sys.argv[3]
    loop=after['sections']['assignment']['assignment']['context']['loop']
    if (after['work']!=work or after['complete'] is not True or after['valid'] is not True
        or after['recipient']!=before['recipient'] or after['binding']==before['binding']
        or loop['state']!='ready' or type(loop['spent']) is not int or loop['spent']!=2
        or type(loop['noInformationStreak']) is not int or loop['noInformationStreak']!=0
        or after['sections']['assignment']['residual']['snapshot']['inputsSha256']
           !=before['sections']['assignment']['residual']['snapshot']['inputsSha256']):
        raise ValueError('current Worker was not safely refreshed after evidence')
except (OSError,ValueError,KeyError,TypeError) as error:
    sys.exit('inventory recovery refused: '+str(error))
PY
    fi
    worker="$tmp/worker-$attempt.json"
    run_product work next "$work" --full --json >"$worker"
    if test "$attempt" = 1; then worker_metric=$(packet_metric "$worker"); fi
    ledger_file=$(find "$fixture" -type f -path '*/runs/*.jsonl' -print | sort | head -n 1)
    test -n "$ledger_file" || { printf '%s\n' 'product fixture ledger unavailable' >&2; exit 2; }
    cp "$ledger_file" "$tmp/ledger-$attempt.jsonl"
    history_events=$(wc -l <"$tmp/ledger-$attempt.jsonl" | tr -d ' ')
    python3 -c 'import json,sys; p=json.load(open(sys.argv[1]))["next"]["packet"]; c=p["context"]; print(json.dumps({"historyEvents":int(sys.argv[2]),"routineExpansionCount":len(c["expansions"]),"routineFieldsBounded":len(c["evidence"])<=8 and len(p["upstreamArtifacts"])<=8},separators=(",",":")))' "$worker" "$history_events" >"$tmp/sample-$attempt.json"
    cat "$tmp/sample-$attempt.json" >>"$tmp/history.jsonl"
    worker_assignment=$(json_field "$worker" next.assignment)
    current_return worker "$worker_assignment" completed 'Record the observed packet and history sample' "$tmp/sample-$attempt.json" "$tmp/worker-detail-$attempt.json" >"$tmp/worker-return-$attempt.json"
    worker_event=$(validate_inventory_submit "$tmp/worker-return-$attempt.json" "$work" "$worker_assignment" completed)
    current_check "$tmp/worker-return-$attempt.json" >"$tmp/check-$attempt.json"
    validate_inventory_check "$tmp/check-$attempt.json" "$work" "$worker_event" >/dev/null
    reviewer="$tmp/reviewer-$attempt.json"
    run_product work next "$work" --full --json >"$reviewer"
    if test "$attempt" = 1; then reviewer_metric=$(packet_metric "$reviewer"); fi
    reviewer_assignment=$(json_field "$reviewer" next.assignment)
    python3 -c 'import json,sys; e=json.load(open(sys.argv[1]))["event"]; print(json.dumps({"eventSha256":e["eventSha256"],"targetEventSha256":e["targetEventSha256"],"result":e["result"]},sort_keys=True))' "$tmp/check-$attempt.json" >"$tmp/reviewer-artifact-$attempt.json"
    current_return reviewer "$reviewer_assignment" approved 'Review the recorded check and observed sample' "$tmp/reviewer-artifact-$attempt.json" "$tmp/reviewer-detail-$attempt.json" >"$tmp/reviewer-return-$attempt.json"
    validate_inventory_submit "$tmp/reviewer-return-$attempt.json" "$work" "$reviewer_assignment" approved >/dev/null
    lead="$tmp/lead-$attempt.json"
    run_product work next "$work" >"$lead"
    if test "$attempt" -lt 3; then
      lead_assignment=$(json_field "$lead" next.assignment)
      python3 -c 'import json,sys; print(json.dumps({"sample":json.load(open(sys.argv[1])),"check":json.load(open(sys.argv[2]))},sort_keys=True))' "$tmp/sample-$attempt.json" "$tmp/reviewer-artifact-$attempt.json" >"$tmp/rework-artifact-$attempt.json"
      current_return lead "$lead_assignment" rework 'Collect the next history sample after this checked cycle' "$tmp/rework-artifact-$attempt.json" "$tmp/rework-detail-$attempt.json" >"$tmp/rework-return-$attempt.json"
      validate_inventory_submit "$tmp/rework-return-$attempt.json" "$work" "$lead_assignment" rework >/dev/null
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
for path in AGENTS.md skills/exitbind-bootstrap/SKILL.md skills/exitbind/SKILL.md skills/exitbind/references/preservation.md skills/exitbind/references/operators.md plugins/exitbind/skills/exitbind/SKILL.md plugins/exitbind/skills/exitbind/references/preservation.md plugins/exitbind/skills/exitbind/references/operators.md .agents/skills/exitbind/SKILL.md .claude/skills/exitbind/SKILL.md exitbind/agents/lead.md exitbind/agents/worker.md exitbind/agents/reviewer.md; do
  if test -f "$path"; then
    total=$((total + $(wc -c <"$path" | tr -d ' ')))
    count=$((count + 1))
  fi
done
printf '\nTotal instruction surface: %s bytes across %s files\n' "$total" "$count"
