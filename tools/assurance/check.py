#!/usr/bin/env python3
"""Fail-closed local/CI assurance checks. Python 3.11+, standard library only."""
from __future__ import annotations
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

DOMAIN = ('convex', 'convex-core', 'convex-math', 'convex-curves', 'convex-bonds', 'convex-analytics', 'convex-portfolio')
FORBIDDEN = {'tokio', 'async-std', 'smol', 'reqwest', 'hyper', 'axum', 'tonic', 'sqlx', 'redb', 'rusqlite', 'postgres', 'tokio-postgres', 'rdkafka', 'lapin', 'async-nats', 'blpapi', 'convex-engine', 'convex-server', 'convex-ports'}
REQUIRED_RUNS = {
    'assurance': (
        'python3 tools/assurance/check.py',
        'python3 -m unittest discover -s tools/assurance -p "test_*.py"',
        'cargo test --locked -p convex-math -p convex-core -p convex-ffi',
        'cargo check --locked -p convex -p convex-core -p convex-math -p convex-curves -p convex-bonds -p convex-analytics -p convex-portfolio --no-default-features',
        'cargo test --locked -p convex-math -p convex-core -p convex-ffi --all-features',
        'cargo test --locked -p convex-curves -p convex-bonds -p convex-analytics',
        'cargo test --locked -p convex-engine --test graph_regressions',
        'cargo build --locked --profile excel -p convex-ffi',
        'python3 tools/assurance/abi_smoke.py',
    ),
    'test': ('cargo test --locked --all-features --workspace -- --test-threads=1',),
    'production-regressions': ('cargo test --locked -p convex-server --test api_integration_tests production_readiness_ -- --include-ignored --test-threads=1',),
    'fmt': ('cargo fmt --all -- --check',),
    'clippy': ('cargo clippy --locked --all-features --workspace -- -D warnings',),
    'docs': ('cargo doc --locked --no-deps --all-features --workspace',),
}

class Failure(ValueError):
    """An explicit assurance rejection; never rely on Python assertions."""

def require(condition: bool, message: str) -> None:
    if not condition:
        raise Failure(message)

def digest(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()

def unique_object(pairs):
    out = {}
    for key, value in pairs:
        require(key not in out, f'duplicate JSON key: {key}')
        out[key] = value
    return out

def read_json(path: Path):
    return json.loads(path.read_text(), object_pairs_hook=unique_object)

def git(root: Path, *args: str) -> bytes:
    return subprocess.check_output(['git', *args], cwd=root)

def git_source(root: Path, revision: str, path: str) -> bytes | None:
    process = subprocess.run(['git', 'show', f'{revision}:{path}'], cwd=root, capture_output=True)
    return process.stdout if process.returncode == 0 else None

def normal_closure(metadata: dict, root_names=DOMAIN) -> set[str]:
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    roots = [p['id'] for p in metadata['packages'] if p['name'] in root_names and p['id'] in metadata['workspace_members']]
    require({packages[i]['name'] for i in roots} == set(root_names), 'missing pure-domain root')
    visited = set()
    pending = list(roots)
    while pending:
        identity = pending.pop()
        if identity in visited:
            continue
        visited.add(identity)
        require(identity in nodes, f'unresolved package: {identity}')
        for dependency in nodes[identity]['deps']:
            # Build scripts/proc macros and dev-only test/benchmark dependencies
            # are not dependencies of calculations. Target-specific normal edges
            # remain included; no host-only filtering can hide an async adapter.
            if any(kind['kind'] is None for kind in dependency['dep_kinds']):
                pending.append(dependency['pkg'])
    return {packages[i]['name'] for i in visited}

def check_dependencies(metadata: dict, allowed: set[str]) -> set[str]:
    names = normal_closure(metadata)
    require(not names & FORBIDDEN, f'forbidden pure-domain dependencies: {sorted(names & FORBIDDEN)}')
    require(names <= allowed, f'unreviewed pure-domain dependencies: {sorted(names - allowed)}')
    return names

def check_workflow(text: str) -> None:
    # This is a narrow check for the committed workflow structure, not a YAML
    # parser/security boundary. Exact workflow bytes are also review protected.
    text = '\n'.join(line for line in text.splitlines() if not line.lstrip().startswith('#'))
    jobs = re.split(r'^  ([a-z][\w-]*):\s*$', text, flags=re.M)
    bodies = dict(zip(jobs[1::2], jobs[2::2]))
    for name, commands in REQUIRED_RUNS.items():
        require(name in bodies, f'missing required CI job: {name}')
        body = bodies[name]
        if name == 'assurance':
            guard = ("      - name: Check explicit change impact against the PR base\n"
                "        if: github.event_name == 'pull_request'\n"
                "        env:\n"
                "          BASE_SHA: ${{ github.event.pull_request.base.sha }}\n"
                '        run: python3 tools/assurance/check.py --base "$BASE_SHA"')
            require(body.count(guard) == 1, 'missing/altered PR base review step')
            body = body.replace(guard, '')
        require(not re.search(r'^\s*(if|continue-on-error|paths|paths-ignore):', body, re.M), f'conditional or ignored required job: {name}')
        runs = re.findall(r'^\s+run: (.+)$', body, re.M)
        for command in commands:
            require(runs.count(command) == 1, f'missing/altered required check in {name}: {command}')
        require('||' not in body and '; true' not in body, f'ignored command exit in {name}')
    require('on:\n  push:\n    branches: [main]\n  pull_request:\n    branches: [main]\n' in text, 'required push/PR triggers changed')
    require("RUSTFLAGS: -Dwarnings" in text, 'CI must deny compiler warnings')
    require("RUSTDOCFLAGS: -Dwarnings" in text, 'CI must deny rustdoc warnings')
    require('paths-ignore:' not in text and not re.search(r'^  paths:', text, re.M), 'required CI cannot have path filters')
    require("CONVEX_PRODUCTION_PROBES: '1'" in bodies['production-regressions'], 'production probes must be activated')


def check_exceptions(document: dict, today: dt.date) -> set[str]:
    require(set(document) == {'schema_version', 'exceptions'} and document['schema_version'] == 1, 'invalid exception schema')
    identifiers = set()
    fields = {'id', 'scope', 'rationale', 'mitigations', 'owner', 'reviewed_on', 'expires_on', 'tracking_task'}
    for row in document['exceptions']:
        require(set(row) == fields, 'unknown or missing exception fields')
        require(row['id'] not in identifiers, 'duplicate exception ID')
        identifiers.add(row['id'])
        require(row['scope'] and all(isinstance(p, str) and p.startswith('crates/') and '..' not in Path(p).parts for p in row['scope']), 'invalid exception scope')
        require(all(isinstance(row[k], str) and row[k].strip() for k in ['rationale', 'mitigations', 'owner', 'tracking_task']), 'exception lacks rationale, owner or mitigation')
        reviewed = dt.date.fromisoformat(row['reviewed_on'])
        expires = dt.date.fromisoformat(row['expires_on'])
        require(reviewed <= today < expires, f'expired/future exception: {row["id"]}')
        require(0 < (expires - reviewed).days <= 90, f'exception exceeds 90 days: {row["id"]}')
    return identifiers


def inventory_paths(root: Path) -> set[str]:
    paths = {str(p.relative_to(root)) for crate in ['convex-math', 'convex-ffi'] for p in (root / 'crates' / crate / 'src').rglob('*.rs')}
    paths.update(f'crates/{crate}/src/lib.rs' for crate in DOMAIN)
    paths.update(['crates/convex-core/src/types/date.rs', 'crates/convex-core/src/types/frequency.rs', 'crates/convex-core/src/calendars/mod.rs', 'crates/convex-core/src/calendars/conventions.rs', 'crates/convex-core/src/calendars/dynamic.rs', 'crates/convex-bonds/src/fixings.rs', 'crates/convex-core/src/daycounts/actact.rs', 'crates/convex-core/src/daycounts/act365.rs', 'crates/convex-core/src/contracts/mod.rs', 'crates/convex-curves/src/curves/discrete.rs', 'crates/convex-curves/src/curves/segmented.rs', 'crates/convex-curves/src/calibration/global_fit.rs', 'crates/convex-analytics/src/calibration/hw1f.rs', 'crates/convex-bonds/src/options/swaption_hw1f.rs', 'crates/convex-engine/src/calc_graph.rs', 'crates/convex-engine/src/runtime/mod.rs'])
    return paths

def check_inventory(root: Path, document: dict, exception_ids: set[str]) -> None:
    require(document['schema_version'] == 1, 'invalid inventory version')
    rows = document['files']
    require(len(rows) == len({r['path'] for r in rows}), 'duplicate inventory path')
    require({r['path'] for r in rows} == inventory_paths(root), 'missing or unexpected critical inventory source')
    for row in rows:
        require(row['sha256'] == digest((root / row['path']).read_bytes()), f'critical source changed without inventory review: {row["path"]}')
        require(row['invariants'] and row['bounds'] and row['failure_contract'], f'incomplete inventory: {row["path"]}')
        require(set(row['exceptions']) <= exception_ids, f'unregistered exception: {row["path"]}')
        for path in row['tests']:
            require((root / path).is_file(), f'missing inventory test: {path}')
    source = (root / 'crates/convex-ffi/src/lib.rs').read_text()
    symbols = re.findall(r'^pub (?:unsafe )?extern "C" fn (convex_\w+)\(', source, re.M)
    require(set(symbols) == set(document['c_exports']) and len(symbols) == len(document['c_exports']), 'C ABI export inventory changed')


def numeric_content(content: bytes, mode: str) -> bytes:
    require(mode in {'file', 'test_section'}, 'invalid numeric evidence mode')
    if mode == 'test_section':
        marker = b'#[cfg(test)]\nmod tests'
        require(marker in content, 'numeric unit-test section missing')
        return content[content.index(marker):]
    return content

def check_numeric(root: Path, document: dict) -> None:
    require(document['schema_version'] == 1 and document['version'] >= 1, 'invalid quantitative evidence version')
    rows = document['artifacts']
    require(len(rows) == len({r['path'] for r in rows}), 'duplicate quantitative artifact')
    required = {'tests/fixtures/quantlib_reference_tests.json', 'reconciliation/reconcile.py', 'reconciliation/ql_bench.py', '.github/workflows/reconcile.yml'}
    require(required <= {r['path'] for r in rows}, 'required comparator/tolerance artifact removed')
    for row in rows:
        content = numeric_content((root / row['path']).read_bytes(), row['mode'])
        require(digest(content) == row['sha256'], f'quantitative evidence changed without version/review: {row["path"]}')
    source = (root / 'crates/convex-math/src/solvers/mod.rs').read_text()
    for name, value in document['solver_defaults'].items():
        require(re.search(rf'pub const {re.escape(name)}:\s*\w+\s*=\s*{re.escape(value)}\s*;', source) is not None, f'reviewed solver default changed: {name}')
    require(document['solver_defaults'] == {'DEFAULT_TOLERANCE': '1e-10', 'DEFAULT_MAX_ITERATIONS': '100'}, 'default changes require explicit quantitative policy revision')


def protected(path: str) -> bool:
    return (path in {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'} or path.startswith('.github/') or path.startswith('tools/assurance/') and path != 'tools/assurance/change-reviews.json' or path == 'docs/ASSURANCE.md' or any(path.startswith(f'crates/{crate}/') for crate in (*DOMAIN, 'convex-ffi')) or path.startswith('tests/fixtures/') or path.startswith('reconciliation/'))

def check_changes(root: Path, base: str, reviews: dict, numeric: dict, today: dt.date) -> int:
    # Include untracked candidate files: local validation cannot miss new modules.
    tracked = git(root, 'diff', '--name-only', '--no-renames', base, '--').decode().splitlines()
    untracked = git(root, 'ls-files', '--others', '--exclude-standard').decode().splitlines()
    changed = {p for p in tracked + untracked if protected(p)}
    require(reviews['schema_version'] == 1, 'invalid change review schema')
    covered = set()
    for row in reviews['reviews']:
        if row['base_commit'] != base:
            continue
        date = dt.date.fromisoformat(row['reviewed_on'])
        require(date <= today and 0 <= (today-date).days <= 90, 'change review expired/future')
        require(row['actor'].strip() and row['rationale'].strip() and row['tests'], 'change impact lacks actor, rationale or tests')
        for path, sha256 in row['candidate_files'].items():
            if path in changed:
                require(path not in covered, f'duplicate change review: {path}')
                candidate = (root / path).read_bytes() if (root / path).is_file() else b''
                require(digest(candidate) == sha256, f'candidate changed after impact review: {path}')
                covered.add(path)
    require(changed == covered, f'unreviewed assurance/numerical changes against {base}: {sorted(changed-covered)}')
    # Refreshing a hash cannot quietly widen or delete old quantitative fixtures.
    # Human identities here are declarations for repository review, not an IdP.
    old_policy = git_source(root, base, 'tools/assurance/numeric-evidence.json')
    if old_policy is not None:
        previous = json.loads(old_policy, object_pairs_hook=unique_object)
        old_rows = {r['path']: r for r in previous['artifacts']}
        new_rows = {r['path']: r for r in numeric['artifacts']}
        changed_pins = any(new_rows.get(path) != row for path, row in old_rows.items())
        if changed_pins:
            require(numeric['version'] > previous['version'], 'quantitative pin revision must increment version')
            approvals = [r for r in reviews['reviews'] if r['base_commit'] == base
                and 'tools/assurance/numeric-evidence.json' in r['candidate_files'] and r.get('quantitative_review')]
            require(approvals, 'quantitative pin removal/refresh needs independent review')
            for row in approvals:
                approval = row['quantitative_review']
                require(approval['reviewer'].strip() and approval['reviewer'] != row['actor']
                    and approval['rationale'].strip() and approval['reference_evidence'], 'pin review needs independent owner/evidence')
    for artifact in numeric['artifacts']:
        old = git_source(root, base, artifact['path'])
        if old is None:
            continue
        current = (root / artifact['path']).read_bytes()
        if numeric_content(old, artifact['mode']) == numeric_content(current, artifact['mode']):
            continue
        approvals = [r for r in reviews['reviews'] if r['base_commit'] == base and artifact['path'] in r['candidate_files'] and r.get('quantitative_review')]
        require(approvals, f'changed quantitative evidence needs owner review: {artifact["path"]}')
        for row in approvals:
            approval = row['quantitative_review']
            require(approval['reviewer'].strip() and approval['reviewer'] != row['actor'] and approval['rationale'].strip() and approval['reference_evidence'], 'quantitative review needs independent owner/evidence')
    return len(changed)


def validate(root: Path, base: str | None, metadata: dict | None = None) -> dict:
    today = dt.datetime.now(dt.timezone.utc).date()
    directory = root / 'tools/assurance'
    exceptions = check_exceptions(read_json(directory / 'exceptions.json'), today)
    for crate in DOMAIN:
        source = (root / f'crates/{crate}/src/lib.rs').read_text()
        require('#![forbid(unsafe_code)]' in source, f'pure crate must forbid unsafe: {crate}')
        require('#![deny(unused_must_use)]' in source, f'pure crate must check results: {crate}')
    for crate in ['convex-math', 'convex-ffi']:
        source = (root / f'crates/{crate}/src/lib.rs').read_text()
        scope = re.search(r'#!\[cfg_attr\(\s*not\(test\),\s*deny\(([^)]*)\)\s*\)\]', source)
        for lint in ['clippy::unwrap_used', 'clippy::expect_used', 'clippy::panic']:
            require(scope is not None and lint in scope.group(1), f'missing production scoped lint: {crate}/{lint}')
    cargo = tomllib.loads((root / 'Cargo.toml').read_text())
    require(cargo['profile']['excel']['panic'] == 'unwind', 'host ABI profile must unwind')
    source = (root / 'crates/convex-ffi/src/lib.rs').read_text()
    require('#[cfg(panic = "abort")]' in source and 'compile_error!' in source, 'ABI cannot silently use abort')
    if metadata is None:
        metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1', '--all-features'], cwd=root))
    dependency_document = read_json(directory / 'dependencies.json')
    names = check_dependencies(metadata, set(dependency_document['allowed_normal_packages']))
    inventory = read_json(directory / 'inventory.json')
    check_inventory(root, inventory, exceptions)
    scope_by_id = {r['id']: set(r['scope']) for r in read_json(directory / 'exceptions.json')['exceptions']}
    for scope in scope_by_id.values():
        require(all((root / p).is_file() for p in scope), 'exception scope has missing source')
    for row in inventory['files']:
        require(all(row['path'] in scope_by_id[i] for i in row['exceptions']), f'exception assigned outside declared scope: {row["path"]}')
    numeric = read_json(directory / 'numeric-evidence.json')
    check_numeric(root, numeric)
    check_workflow((root / '.github/workflows/ci.yml').read_text())
    changes = check_changes(root, base, read_json(directory / 'change-reviews.json'), numeric, today) if base else None
    return {'status': 'passed', 'normal_packages': len(names), 'exceptions': len(exceptions), 'reviewed_changes': changes, 'quantitative_artifacts': len(numeric['artifacts'])}

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--base', help='Git revision for explicit change-impact review; required in PR CI')
    args = parser.parse_args()
    try:
        print(json.dumps(validate(args.root.resolve(), args.base), sort_keys=True))
        return 0
    except (Failure, OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(f'assurance rejected: {error}', file=sys.stderr)
        return 1

if __name__ == '__main__':
    sys.exit(main())
