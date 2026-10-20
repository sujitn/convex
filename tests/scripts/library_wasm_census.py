"""Compare a complete frozen native census with actual wasm32/Node responses."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def run(index, wasm, bindgen, output):
    index, wasm, bindgen, output = (Path(p).resolve() for p in (index, wasm, bindgen, output))
    if output.exists():
        raise ValueError('Refuse replacement of actual WASM evidence')
    output.mkdir(parents=True)
    lock = tomllib.loads((ROOT / 'Cargo.lock').read_text())
    version = next(p['version'] for p in lock['package'] if p['name'] == 'wasm-bindgen')
    if subprocess.check_output([str(bindgen), '--version'], text=True).strip() != 'wasm-bindgen ' + version:
        raise ValueError('Generated glue requires the exact locked bindgen version')
    subprocess.run([str(bindgen), str(wasm), '--target', 'nodejs', '--out-dir', str(output / 'node')], check=True)
    command = ['node', str(ROOT / 'tests/scripts/library_wasm_census.cjs'),
               str(output / 'node/convex_wasm.js'), str(index), str(output / 'actual')]
    with (output / 'node.stdout').open('xb') as stdout, (output / 'node.stderr').open('xb') as stderr:
        result = subprocess.run(command, stdout=stdout, stderr=stderr, cwd=ROOT)
    summary = json.loads((output / 'actual/summary.json').read_bytes())
    summary.update(wasm_binary_sha256=sha(wasm), bindgen_sha256=sha(bindgen),
                   locked_bindgen_version=version, native_index_sha256=sha(index),
                   host_source_pins={p: sha(ROOT / p) for p in
                                     ['tests/scripts/library_wasm_census.py', 'tests/scripts/library_wasm_census.cjs']},
                   generated_host_files={p.relative_to(output).as_posix(): sha(p)
                                         for p in (output / 'node').iterdir() if p.is_file()})
    (output / 'qualification.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps({k: v for k, v in summary.items() if k not in ['corpora', 'differences', 'generated_host_files']}))
    if result.returncode or summary['status'] != 'passed':
        raise ValueError('Full actual wasm32 census differs; inspect retained complete responses and summary')
    return summary


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['index', 'wasm', 'bindgen', 'output']:
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    run(args.index, args.wasm, args.bindgen, args.output)
