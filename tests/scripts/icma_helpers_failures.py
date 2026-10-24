"""Separate original boundary/negative admission using complete independent cash."""
import argparse
from copy import deepcopy
from decimal import Decimal as D
import json
from pathlib import Path
from icma_helpers_reference import quote_price
from icma_reference import day, fraction
ROOT=Path(__file__).resolve().parents[2]
DEST=ROOT/'tests/fixtures/library'

def qualify():
    cases=deepcopy(json.loads((DEST/'icma-helpers-ready-api.json').read_bytes())['cases'])
    answers=[]
    # The original large negative coupon observation has a negative dirty cash
    # price. The existing cash-helper contract rejects it before yield inversion.
    next(c for c in cases if c['id']=='icma_helpers.invalid.mixed_sign')['expected_error']='invalid_input'
    # Keep the two malformed raw ex-date requests as strict ingress negatives.
    for c in cases[:]:
        if c['id'] not in ['icma_helpers.boundary.pre_ex','icma_helpers.boundary.on_ex']:
            continue
        actual=deepcopy(c);actual['id']+='.admitted'
        h=actual['request']['operation']['input']['helper']
        h['terms']['dates']['ex_coupon']=dict(offset={'CalendarDays':-3},anchor='PaymentDate',boundary='OnDate')
        cases.append(actual);c['expected_error']='invalid_input'
    # Original accrued resets on the payment date, including an owned same-day
    # coupon; future-period accrued remains. Recompute the entire independent
    # quotation, retaining the raw original incorrect full-coupon accrued answer.
    c=next(c for c in cases if c['id']=='icma_helpers.boundary.payment_include')
    h=c['request']['operation']['input']['helper'];mesh=h['quote_kind']['input']['reference_context']['periods']
    accrued=D('.048')*100*fraction(day('2024-02-29'),day(h['settlement']),mesh,2)
    corrected=quote_price(h,deepcopy(c['expected']['observed']['cashflows']),accrued,D('.04125'))
    answers.append(dict(id=c['id'],expected=dict(observed=corrected)))
    # Add a positive-price mixed-sign observation to reach the original inverse
    # guard rather than failing the cash-helper's positive observation contract.
    c=deepcopy(next(c for c in cases if c['id']=='icma_helpers.invalid.mixed_sign'))
    c['id']='icma_helpers.invalid.positive_price_mixed_sign'
    c['request']['operation']['input']['input']['recipe']['helpers'][0]['input']['terms']['profiles']['coupon'][0]['value']='-0.025'
    c['expected_error']='unsupported';cases.append(c)
    c=deepcopy(next(c for c in cases if c['id']=='icma_helpers.regular.SemiAnnual.0.04125.convert'))
    c['id']='icma_helpers.invalid.convert_context'
    c['request']['operation']['input']['helper']['quote_kind']['input']['reference_context']['periods']=[]
    c['expected_error']='invalid_input';cases.append(c)
    c=deepcopy(next(c for c in cases if c['id']=='icma_helpers.graph.positive.build'))
    c['id']='icma_helpers.invalid.issuer_security_revision'
    c['request']['operation']['input']['cash'][0]['recipe']['helpers'][0]['input']['credit_identity']['instrument_version']=99
    c['expected_error']='invalid_input';cases.append(c)
    return dict(schema_version=1,certified=False,qualification='All preceding raw requests/answers remain physically unchanged. Separate exact payment-date accrued reset and full ex-date rule controls use unchanged dirty/PV/cash/time/yield/invoice and declared bounds. Two malformed ex-date requests remain negatives; the negative-dirty observation fails positive cash-price admission before inversion. Genuine positive-price mixed-sign, missing conversion context and independent issuer revision failures are separately declared. No native outputs become financial controls.',cases=cases),dict(certified=False,cases=answers)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();values=qualify()
    for name,value in zip(['icma-helpers-final-api.json','icma-helpers-final-reference.json'],values):
        path=DEST/name;data=(json.dumps(value,indent=2)+'\n').encode()
        if a.write:
            if path.exists():raise ValueError('Refusing to overwrite independent boundary admission')
            path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Original independent admission changed')
    print(json.dumps(dict(requests=len(values[0]['cases']),successes=sum(c['expected_error'] is None for c in values[0]['cases']),certified=False)))
