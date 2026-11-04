"""Separate security-alias and existing product-role admission; no financial edits."""
import argparse, copy, datetime, hashlib, json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
DEST=ROOT/'tests/fixtures/library'

def qualify():
    prior=json.loads((DEST/'icma-helpers-terms.json').read_bytes())
    refs=[]
    for name in ['reference','additional-reference','terms-reference']:
        refs.extend(json.loads((DEST/('icma-helpers-'+name+'.json')).read_bytes())['cases'])
    cases=copy.deepcopy(prior['cases']);answers=[]
    # The raw Python declaration shared the security object with its context.
    # Updating that one alias changed BOTH pins. This is a valid newly supplied
    # matching revision, never evidence of registry freshness or approval.
    case=next(c for c in cases if c['id']=='icma_helpers.invalid.security_revision')
    case['expected_error']=None
    answer=copy.deepcopy(next(r for r in refs if r['id']=='icma_helpers.regular.SemiAnnual.0.04125.reprice'))
    answer['id']=case['id'];answers.append(answer)
    bad=copy.deepcopy(case);bad['id']='icma_helpers.invalid.independent_security_mismatch'
    bad['request']['operation']['input']['input']['recipe']['helpers'][0]['input']['quote_kind']['input']['reference_context']['security']['version']=17
    bad['expected_error']='invalid_input';cases.append(bad)
    for case in cases[:]:
        if not case['id'].endswith('.risk.admitted.terms'):
            continue
        actual=copy.deepcopy(case);actual['id']+='.role'
        role=actual['request']['operation']['action']['product']['method']['role']
        role['sovereign']=role.pop('benchmark_id')
        cases.append(actual)
        answer=copy.deepcopy(next(r for r in refs if r['id']==case['id']))
        answer['id']=actual['id'];answers.append(answer)
        case['expected_error']='invalid_input'
    qualified=dict(certified=False,qualification='Every original request/answer/domain/bound is retained. The raw aliased pair of matching security revision99 pins is a valid supplied revision with unchanged full independent economics; a separate one-pin mismatch is declared. Three strict government product-role benchmark_id failures remain and three complete sovereign requests use the identical government identity and original five-PV/risk controls. No external source freshness or certificate is implied.',cases=cases)
    api=copy.deepcopy(cases)+json.loads((DEST/'icma-helpers-boundaries.json').read_bytes())['cases']
    return qualified,dict(certified=False,cases=answers),dict(schema_version=1,certified=False,cases=api)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();values=qualify()
    names=['icma-helpers-ready.json','icma-helpers-ready-reference.json','icma-helpers-ready-api.json']
    for name,value in zip(names,values):
        path=DEST/name;data=(json.dumps(value,indent=2)+'\n').encode()
        if a.write:
            if path.exists():raise ValueError('Refusing to overwrite independent admission')
            path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Independent admission changed')
    print(json.dumps(dict(requests=len(values[2]['cases']),successes=sum(c['expected_error'] is None for c in values[2]['cases']),certified=False)))
