"""Independent completed-but-unpaid purchase accrual boundary controls."""
import argparse
from copy import deepcopy
from decimal import Decimal as D
import json
from pathlib import Path
from icma_reference import coupon, day

ROOT=Path(__file__).resolve().parents[2]
def references():
    cases=json.loads((ROOT/'tests/fixtures/library/icma-qualified.json').read_bytes())['cases']
    refs=json.loads((ROOT/'tests/fixtures/library/icma-additional-reference.json').read_bytes())['cases']
    rows=[]
    for c in cases:
        if not c['id'].startswith('icma.qualified.boundary.'):continue
        r=deepcopy(next(r for r in refs if r['id']==c['id']));op=c['request']['operation'];t=op['input']['security']['bond']['terms']
        # The ORIGINAL first coupon completed at purchase but pays on Mar12.
        # Its full earned receivable remains in purchase accrued until payment.
        full=coupon(t,day('2023-08-31'),day('2024-02-29'),op['input']['reference_context']['periods'],2)
        accrued=D(r['expected']['accrued_per100'])+full/D(t['face_value'])*100
        r['expected']['accrued_per100']=str(accrued)
        r['expected']['clean_per100']=str(D(r['expected']['dirty_per100'])-accrued)
        rows.append(r)
    if len(rows)!=6:raise ValueError('Original purchase boundary scope changed')
    return dict(certified=False,qualification='Six separately declared original purchase boundary controls explicitly include the completed-but-unpaid coupon in accrued. Original raw/admitted under-accrual diagnostics remain unchanged. Dirty/PV/payment times/cash/yield/inverse domain/invoice answers are byte-value unchanged; same original bounds.',cases=rows)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();value=references();file=ROOT/'tests/fixtures/library/icma-entitlement-reference.json'
    if a.write:
        if file.exists():raise ValueError('Refusing to overwrite declared entitlement controls')
        file.write_text(json.dumps(value,indent=2)+'\n')
    elif json.loads(file.read_bytes())!=value:raise ValueError('Original entitlement controls changed')
    print(json.dumps(dict(entitlement_controls=6,original_dirty_pv_cash_yield_invoice_preserved=True,certified=False)))
