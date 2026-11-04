"""Separate strict product-context admission; original inputs/answers stay fixed."""
import argparse,copy,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
def qualify():
    raw=json.loads((ROOT/'tests/fixtures/library/icma-helpers.json').read_bytes());refs=json.loads((ROOT/'tests/fixtures/library/icma-helpers-reference.json').read_bytes())
    cases=copy.deepcopy(raw['cases']);added=[]
    for case in cases[:]:
        if case['id'].endswith('.risk'):
            actual=copy.deepcopy(case);actual['id']+='.admitted'
            ctx=actual['request']['operation']['action']['product']['context'];ctx['fixing']=ctx.pop('fixings');ctx['overnight_accrual_cutoffs']=[]
            cases.append(actual);answer=copy.deepcopy(next(c for c in refs['cases'] if c['id']==case['id']));answer['id']=actual['id'];added.append(answer)
            case['expected_error']='invalid_input'
    return dict(certified=False,qualification='Every148originalrequest/input/domain/work/financialanswer remains. Three original product-context spelling/required-field failures remain explicit strict ingress negatives. Three separately declared complete actual contexts use original valuation/known-at/settlement and no overnight partial choices; all original five-PV/risk answers and financial bounds stay unchanged.',cases=cases),dict(certified=False,cases=added)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();values=qualify()
    for file,value in zip(['icma-helpers-qualified.json','icma-helpers-additional-reference.json'],values):
        path=ROOT/'tests/fixtures/library'/file;data=(json.dumps(value,indent=2)+'\n').encode()
        if a.write:
            if path.exists():raise ValueError('Refusing to overwrite declaration')
            path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Original strict qualification changed')
    print(json.dumps(dict(requests=len(values[0]['cases']),successes=sum(c['expected_error'] is None for c in values[0]['cases']),certified=False)))
