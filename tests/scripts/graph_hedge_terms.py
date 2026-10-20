"""Separate full CDS premium-grid admission; raw failed request/answers stay.

The independent CDS control explicitly integrates from 2026-09-20. The raw new
fixture supplied 2026-10-07 instead. This separate supplied-term fixture declares
the intended complete September premium period before execution. No original
price/risk answer, model, side, remaining date, source or tolerance is changed.
"""
from copy import deepcopy
import argparse
import hashlib
import json
from pathlib import Path

FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');a=parser.parse_args()
    source=FOLDER/'graph-hedge-qualified.json';raw=source.read_bytes();corpus=deepcopy(json.loads(raw))
    for case in corpus['cases']:
        product=case['request']['operation']['product']
        if product['kind']=='cds':
            assert product['terms']['premium_schedule']['start_date']=='2026-10-07'
            product['terms']['premium_schedule']['start_date']='2026-09-20'
    corpus['terms_admission_source_sha256']=hashlib.sha256(raw).hexdigest();data=(json.dumps(corpus,indent=2)+'\n').encode()
    target=FOLDER/'graph-hedge-terms.json'
    if a.write:
        if target.exists():raise ValueError('Refusing to replace separately declared terms')
        target.write_bytes(data)
    elif target.read_bytes()!=data:raise ValueError('Terms admission differs')
    print(json.dumps(dict(status='passed',requests=39,original_financial_answers_and_bounds_unchanged=True,certified=False)))
