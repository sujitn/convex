"""Lossless full residual/partial receipt request census at original wire bounds."""
import argparse,hashlib,json
from pathlib import Path
from delivery_hedge_census import compact,spans
from causal_pnl_census import descriptor
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
SOURCE='residual-pnl-canonical.json'
def source_cases():
    return json.loads((FOLDER/SOURCE).read_bytes())['cases']
def build():
    raw=(FOLDER/SOURCE).read_bytes();cases=source_cases()
    prefix=json.dumps(dict(schema_version=1,certified=False,source_sha256=hashlib.sha256(raw).hexdigest(),
        qualification='Whole original financial requests/outcomes. Original independent full values, receipt ownership and signed face controls remain separate. Native envelopes prove transport only.'))[:-1]+',"cases":['
    parts=[];pending=[]
    for request in [descriptor(c) for c in spans(raw.decode(),'cases')]:
        if pending and len(compact((prefix+','.join(pending+[request])+']}').encode()))>2*1024*1024:
            parts.append(compact((prefix+','.join(pending)+']}').encode()));pending=[]
        pending.append(request)
    if pending:parts.append(compact((prefix+','.join(pending)+']}').encode()))
    if any(len(p)>2*1024*1024 for p in parts):raise ValueError('Original request limit exceeded')
    if [c for p in parts for c in json.loads(p)['cases']]!=[{k:c[k] for k in ['id','request','expected_error']} for c in cases]:
        raise ValueError('Original request/outcome census changed')
    return parts
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    parts=build();paths=[FOLDER/f'residual-pnl-canonical-api-{i+1:02d}.json' for i in range(len(parts))]
    if a.write and any(p.exists() for p in paths):raise ValueError('Refusing overwrite')
    for path,data in zip(paths,parts):
        if a.write:path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Original residual corpus bytes changed')
    print(json.dumps(dict(status='passed',cases=len(source_cases()),parts=len(parts),certified=False)))
