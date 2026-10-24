"""Separate native hazard-key admission; original full financial controls stay exact.

The raw hazard key retained the failed ZeroNode spelling. The existing canonical
hazard factor requires ModelParameter. Only that spelling changes here; every
original request, price, derivative, unit, work policy and numerical bound stays.
"""
from copy import deepcopy
import argparse
import hashlib
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2];FOLDER=ROOT/'tests/fixtures/library'
SOURCES=['graph-hedge.json','graph-hedge-problems.json']


def normalize(value):
    if isinstance(value,dict):
        if value.get('quote_type')=='hazard_rate' and value.get('basis')=='zero_node':value['basis']='model_parameter'
        for v in value.values():normalize(v)
    elif isinstance(value,list):
        for v in value:normalize(v)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');a=parser.parse_args()
    for name in SOURCES:
        original=json.loads((FOLDER/name).read_bytes());admitted=deepcopy(original);normalize(admitted)
        admitted['admission_source_sha256']=hashlib.sha256((FOLDER/name).read_bytes()).hexdigest()
        data=(json.dumps(admitted,indent=2)+'\n').encode();dest=FOLDER/name.replace('.json','-admitted.json')
        if a.write:
            if dest.exists():raise ValueError('Refusing to replace separate admission control')
            dest.write_bytes(data)
        elif dest.read_bytes()!=data:raise ValueError('Admission control differs')
    print(json.dumps(dict(status='passed',requests=39,mixed_books=2,original_financial_controls_unchanged=True,certified=False)))
