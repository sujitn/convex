"""Lossless complete original cohort corpus plus full five-cause financial book."""
import argparse,hashlib,json
from delivery_hedge_census import compact,spans
from causal_pnl_census import descriptor
from cohort_pnl_census import FOLDER,source_cases as earlier
SOURCE='cohort-pnl-full-mixed.json'
def source_cases():
    cases=earlier()+json.loads((FOLDER/SOURCE).read_bytes())['cases']
    if len(cases)!=41 or len({c['id'] for c in cases})!=41:raise ValueError('Complete original cohort scope differs')
    return cases
def build():
    from cohort_pnl_census import SOURCE as prior
    raws=[(FOLDER/name).read_bytes() for name in [prior,SOURCE]]
    original=[descriptor(c) for raw in raws for c in spans(raw.decode(),'cases')]
    prefix=json.dumps(dict(schema_version=1,certified=False,source_sha256=[hashlib.sha256(raw).hexdigest() for raw in raws],qualification='Whole original41 cohort/physical and full five-cause requests/outcomes. Independent full cash/price/CTD/attribution controls and all failures remain separate; native envelopes are transport only.'))[:-1]+',"cases":['
    parts=[];pending=[]
    for request in original:
        if pending and len(compact((prefix+','.join(pending+[request])+']}').encode()))>2*1024*1024:
            parts.append(compact((prefix+','.join(pending)+']}').encode()));pending=[]
        pending.append(request)
    if pending:parts.append(compact((prefix+','.join(pending)+']}').encode()))
    expected=[{k:c[k] for k in ['id','request','expected_error']} for c in source_cases()]
    if [c for p in parts for c in json.loads(p)['cases']]!=expected:raise ValueError('Full original request changed')
    if any(len(p)>2*1024*1024 for p in parts):raise ValueError('Original corpus bound exceeded')
    return parts
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();parts=build();paths=[FOLDER/f'cohort-pnl-complete-api-{i+1:02d}.json' for i in range(len(parts))]
    if a.write and any(p.exists() for p in paths):raise ValueError('Refusing overwrite')
    for path,data in zip(paths,parts):
        if a.write:path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Lossless exact complete cohort corpus differs')
    print(json.dumps(dict(status='passed',cases=41,parts=len(parts),certified=False)))
