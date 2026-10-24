"""Separate full-disposal Money unit spelling; no financial value changes.

Preserve the failed full mixed request/reference and writer. The supplied actual
consideration and fees explicitly carry the original currency unit, exactly like
all other original Money fields. No financial answer, source, amount or bound is
changed or replaced; this admission precedes the next native execution.
"""
import argparse,hashlib,json
from pathlib import Path
from cohort_pnl_wire import units
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
SOURCE=FOLDER/'cohort-pnl-full-mixed.json';TARGET=SOURCE.with_name('cohort-pnl-full-wire.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for case in data['cases']:units(case['request'])
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original full signed Money unit declaration differs')
    print(json.dumps(dict(status='passed',cases=1,positive=1,certified=False)))
