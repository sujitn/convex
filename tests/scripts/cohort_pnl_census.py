"""Lossless bounded whole original cohort request/outcome descriptors."""
import argparse,hashlib,json
from pathlib import Path
from delivery_hedge_census import compact,spans
from causal_pnl_census import descriptor
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
SOURCE='cohort-pnl-calendars.json'
def source_cases():
    cases=json.loads((FOLDER/SOURCE).read_bytes())['cases']
    if len(cases)!=40 or len({c['id'] for c in cases})!=40:raise ValueError('Complete cohort declaration changed')
    return cases
def build():
    raw=(FOLDER/SOURCE).read_bytes()
    original=[descriptor(c) for c in spans(raw.decode(),'cases')]
    prefix=json.dumps(dict(schema_version=1,certified=False,source_sha256=hashlib.sha256(raw).hexdigest(),qualification='Complete cohort/cash/physical requests and outcomes. Independent full monetary/model/cash/CTD/P&L controls and every failure remain separate; native transport is not financial approval.'))[:-1]+',"cases":['
    parts=[];pending=[]
    for request in original:
        if pending and len(compact((prefix+','.join(pending+[request])+']}').encode()))>2*1024*1024:
            parts.append(compact((prefix+','.join(pending)+']}').encode()));pending=[]
        pending.append(request)
    if pending:parts.append(compact((prefix+','.join(pending)+']}').encode()))
    expected=[{k:c[k] for k in ['id','request','expected_error']} for c in source_cases()]
    if [c for p in parts for c in json.loads(p)['cases']]!=expected:raise ValueError('Full original request changed')
    if any(len(p)>2*1024*1024 for p in parts):raise ValueError('Original corpus-file bound exceeded')
    return parts
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();parts=build();paths=[FOLDER/f'cohort-pnl-api-{i+1:02d}.json' for i in range(len(parts))]
    if a.write and any(p.exists() for p in paths):raise ValueError('Refusing overwrite')
    for path,data in zip(paths,parts):
        if a.write:path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Lossless exact cohort corpus differs')
    print(json.dumps(dict(status='passed',cases=40,parts=len(parts),bytes=[len(p) for p in parts],certified=False)))
