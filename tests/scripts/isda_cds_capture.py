#!/usr/bin/env python3
"""Reproduce original ISDA 1.8.3 grid execution using a supplied publisher ZIP.

No network, installation or production linkage. Requires clang/clang++ and a
checksum-matching original source archive, downloaded by the operator. Original
source bytes are checked and used without edits. Output directory must be new.
"""
from __future__ import annotations
import argparse
import datetime as dt
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile
from isda_cds_compare import ROOT, PACK, DRIVER, SOURCE_SHA, digest, reference, require, Failure, compare


def capture(archive, output):
    require(digest(archive.read_bytes()) == SOURCE_SHA, 'publisher source ZIP checksum differs')
    grid, _, _ = reference()
    require(not output.exists(), 'refusing to overwrite an original execution')
    output.mkdir(parents=True)
    output = output.absolute()
    (output/'rates.txt').write_text(''.join(f"{q['tenor']} {q['decimal_rate']}\n" for q in grid['rate_quotes']))
    (output/'cases.txt').write_text(''.join(f"{c['row']} {' '.join(c['maturity_date'].split('-'))} {float(c['coupon_bps'])/10000:.17g} {float(c['spread_bps'])/10000:.17g} {c['recovery']}\n" for c in grid['cases']))
    commands = []
    def run(label, argv):
        started = dt.datetime.now(dt.timezone.utc).isoformat()
        with (output/(label+'.stdout')).open('wb') as stdout, (output/(label+'.stderr')).open('wb') as stderr:
            result = subprocess.run([str(v) for v in argv], cwd=ROOT, stdout=stdout, stderr=stderr, timeout=60)
        commands.append(dict(label=label, argv=list(map(str, argv)), started_at=started,
            finished_at=dt.datetime.now(dt.timezone.utc).isoformat(), exit_code=result.returncode,
            stdout_sha256=digest((output/(label+'.stdout')).read_bytes()),
            stderr_sha256=digest((output/(label+'.stderr')).read_bytes())))
        (output/'commands.json').write_text(json.dumps(commands, indent=2)+'\n')
        require(result.returncode == 0, f'original subprocess failed: {label}')
    with tempfile.TemporaryDirectory(prefix='convex-isda183-') as tmp:
        build = Path(tmp)
        with zipfile.ZipFile(archive) as source:
            entries = source.infolist()
            require(len(entries) <= 512 and sum(e.file_size for e in entries) <= 32*1024*1024,
                    'publisher archive work bound exceeded')
            require(len({e.filename for e in entries}) == len(entries), 'duplicate source ZIP member')
            for entry in entries:
                require(not Path(entry.filename).is_absolute() and '..' not in Path(entry.filename).parts,
                        'invalid publisher path')
            source.extractall(build)
            model = build/'isda_cds_model_c_v1.8.3'
            source_files = {str(p.relative_to(model)): digest(p.read_bytes()) for p in sorted(model.rglob('*')) if p.is_file()}
            for entry in entries:
                if not entry.is_dir():
                    require((build/entry.filename).read_bytes() == source.read(entry), 'original source bytes changed')
        include = model/'lib/include/isda'
        objects = build/'objects'
        objects.mkdir()
        run('compiler', ['clang', '--version'])
        sources = sorted((model/'lib/src').glob('*.c'))
        require(len(sources) == 48, 'original source compile scope differs')
        for index, source in enumerate(sources):
            run(f'compile-{index:02d}', ['clang','-O2','-include','ldate.h','-DUNIX','-DLINUX','-DVERSION="1.8.3"',
                '-I'+str(include),'-c',source,'-o',objects/(source.stem+'.o')])
        run('compile-cpp', ['clang++','-O2','-DUNIX','-DLINUX','-DVERSION="1.8.3"','-I'+str(include),'-c',model/'lib/src/cfinanci.cpp','-o',objects/'cfinanci.o'])
        run('compile-driver', ['clang','-O2','-I'+str(include),'-c',ROOT/DRIVER,'-o',objects/'driver.o'])
        binary = build/'capture'
        run('link', ['clang++',*[objects/(s.stem+'.o') for s in sources],objects/'cfinanci.o',objects/'driver.o','-lm','-o',binary])
        binary_sha = digest(binary.read_bytes())
        run('execution', [binary,output/'rates.txt',output/'cases.txt',ROOT/PACK/'NYM.csv'])
    actual = json.loads((output/'execution.stdout').read_bytes())
    require(actual['model_version'] == 'CDS version "1.8.3"', 'executed model version differs')
    result = compare(actual['cases'], grid)
    (output/'report.json').write_text(json.dumps(result, indent=2)+'\n')
    (output/'manifest.json').write_text(json.dumps(dict(source_sha256=SOURCE_SHA, source_files=source_files,
        driver_sha256=digest((ROOT/DRIVER).read_bytes()), binary_sha256=binary_sha, executed=True,
        certified=False, output_sha256=digest((output/'execution.stdout').read_bytes())), indent=2)+'\n')
    require(result['status'] == 'passed', 'actual original model failed published precision')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-archive', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(capture(args.source_archive, args.output)))
        return 0
    except (Failure, ValueError, OSError, subprocess.TimeoutExpired) as error:
        print(json.dumps(dict(status='execution_failed', error=str(error), certified=False)))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
