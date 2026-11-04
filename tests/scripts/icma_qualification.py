"""Separate original option-guard diagnostics and genuinely plain controls."""
import argparse
from copy import deepcopy
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
def qualified():
    raw=json.loads((ROOT/'tests/fixtures/library/icma-quotations.json').read_bytes())
    refs=json.loads((ROOT/'tests/fixtures/library/icma-reference.json').read_bytes())
    cases=deepcopy(raw['cases']);additional=[]
    for c in cases[:]:
        if c['expected_error'] is None and c['id'].startswith(('icma.stepped.','icma.boundary.')):
            admitted=deepcopy(c);admitted['id']='icma.qualified.'+c['id'].removeprefix('icma.')
            admitted['request']['operation']['input']['security']['bond']['terms']['exercise']=[]
            answer=deepcopy(next(r for r in refs['cases'] if r['id']==c['id']));answer['id']=admitted['id']
            cases.append(admitted);additional.append(answer)
            c['expected_error']='unsupported'
    if len(additional)!=12:raise ValueError('Original declared option/plain distinction changed')
    return dict(certified=False,qualification='All original135 requests retained;12 original option-bearing plain requests are unsupported diagnostics. Twelve separately declared plain contracts omit those rights, with unchanged coupon/principal/redemption/accrual/quote/inverse/quantity controls. No optional-model guard or original answer/bound is changed.',cases=cases),dict(certified=False,cases=additional)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();values=qualified()
    for path,value in zip(['tests/fixtures/library/icma-qualified.json','tests/fixtures/library/icma-additional-reference.json'],values):
        file=ROOT/path
        if a.write:
            if file.exists():raise ValueError('Refusing to overwrite declared qualification')
            file.write_text(json.dumps(value,indent=2)+'\n')
        elif json.loads(file.read_bytes())!=value:raise ValueError('Original qualification changed')
    print(json.dumps(dict(cases=len(values[0]['cases']),successes=sum(c['expected_error'] is None for c in values[0]['cases']),typed_failures=sum(c['expected_error'] is not None for c in values[0]['cases']),separate_plain_controls=12,certified=False)))
