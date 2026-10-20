"""Independent explicit decomposition of original combined workout payables."""
import argparse
from copy import deepcopy
from decimal import Decimal as D
import json
from pathlib import Path
from icma_reference import day, profile, coupon, PER_YEAR

ROOT=Path(__file__).resolve().parents[2]
def components():
    cases=json.loads((ROOT/'tests/fixtures/library/icma-qualified.json').read_bytes())['cases']
    refs=json.loads((ROOT/'tests/fixtures/library/icma-reference.json').read_bytes())['cases']
    out=[]
    for c in cases:
        op=c['request']['operation'];selection=op['cash']
        if c['expected_error'] is not None or selection['kind']!='workout' or selection['selection']['kind']!='exercise':continue
        original=deepcopy(next(r for r in refs if r['id']==c['id']))
        flows=original['expected']['cashflows'];combined=flows[-1]
        t=op['input']['security']['bond']['terms'];exercise=day(selection['selection']['date'])
        mesh=op['input']['reference_context']['periods'];f=PER_YEAR[t['schedule']['frequency']]
        # These declared cases are in the ORIGINAL Feb29..Aug31 coupon. The
        # completed February coupon retains its original later payment date.
        partial=coupon(t,day('2024-02-29'),exercise,mesh,f)
        provision=t['exercise'][selection['selection']['provision']]
        redemption=profile(t,'notional',exercise)*D(provision['price_per_100'])/100
        if D(combined['amount'])!=redemption+partial:raise ValueError('Original combined payable changed')
        flows[-1:]=[dict(date=combined['date'],kind='coupon',amount=str(partial)),dict(date=combined['date'],kind='principal',amount=str(redemption))]
        flows.sort(key=lambda c:(c['date'],c['kind']))
        out.append(original)
    if len(out)!=8:raise ValueError('Original conditional control scope changed')
    return dict(certified=False,qualification='Original combined accrued-plus-redemption payables retain EXACT total/date/PV/price/yield/invoice answers. Independent explicit coupon/principal component controls disclose original decomposition. Original combined references and failed diagnostic remain unchanged.',cases=out)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();value=components();file=ROOT/'tests/fixtures/library/icma-cash-components.json'
    if a.write:
        if file.exists():raise ValueError('Refusing to overwrite declared component controls')
        file.write_text(json.dumps(value,indent=2)+'\n')
    elif json.loads(file.read_bytes())!=value:raise ValueError('Original component controls changed')
    print(json.dumps(dict(component_controls=8,all_original_combined_payables_preserved=True,certified=False)))
