"""Run complete fresh native envelopes against actual wasm32/Node JS glue.

Match the wasm-bindgen CLI to the repository's exact locked package. Retain actual
native requests/envelopes and wasm host answers. Never use native transport output
as an independent financial oracle. No numeric tolerance or model approximation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def parity(binary, wasm, bindgen, output):
    output = Path(output).resolve()
    if output.exists():
        raise ValueError('Refusing overwrite of actual host evidence')
    output.mkdir(parents=True)
    lock = tomllib.loads((ROOT / 'Cargo.lock').read_text())
    version = next(p['version'] for p in lock['package'] if p['name'] == 'wasm-bindgen')
    observed = subprocess.check_output([bindgen, '--version'], text=True).strip()
    if observed != 'wasm-bindgen ' + version:
        raise ValueError('CLI differs from locked wasm-bindgen package')
    native = output / 'native-envelopes.json'
    raw = subprocess.check_output([binary, '--emit-parity'])
    data = json.loads(raw)
    if data['certified'] is not False or not data['cases']:
        raise ValueError('Full uncertified transport source required')
    native.write_bytes(raw)
    subprocess.run([bindgen, wasm, '--target', 'nodejs', '--out-dir', str(output / 'node')], check=True)
    result = subprocess.run(['node', str(ROOT / 'tests/scripts/library_wasm_parity.cjs'),
        str(output / 'node/convex_wasm.js'), str(native), str(output / 'wasm-envelopes.json')],
        capture_output=True, text=True)
    (output / 'node.stdout').write_text(result.stdout)
    (output / 'node.stderr').write_text(result.stderr)
    if result.returncode:
        print(result.stderr[-4000:])
        raise ValueError('Actual wasm32/Node envelope parity failed')
    summary = json.loads(result.stdout.strip().splitlines()[-1])
    summary.update(native_binary_sha256=sha(binary), wasm_binary_sha256=sha(wasm),
        bindgen_sha256=sha(bindgen), locked_bindgen_version=version,
        native_envelopes_sha256=sha(native), wasm_envelopes_sha256=sha(output / 'wasm-envelopes.json'),
        generated_host_files={str(p.relative_to(output)): sha(p) for p in (output / 'node').iterdir() if p.is_file()})
    return summary


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    for argument in ['binary', 'wasm', 'bindgen', 'output']:
        parser.add_argument('--' + argument, required=True)
    args = parser.parse_args()
    print(json.dumps(parity(args.binary, args.wasm, args.bindgen, args.output)))
