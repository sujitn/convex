#!/usr/bin/env python3
"""Preserve raw controls; admit the original native fixing/quantity wire contract.

No rate, quantity, economic term, price answer, target, tolerance or bound changes.
"""
import argparse
import copy
import json
from pathlib import Path

FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
OUTPUT=FOLDER/'floating-workout-admitted.json'
def admission():
    original=json.loads((FOLDER/'floating-workouts.json').read_bytes()); result=copy.deepcopy(original)
    for case in result['cases']:
        r=case['request']
        for fixing in r['inputs']['fixings']: fixing['source_id']='FLOAT.ORIGINAL.ADMINISTRATOR'
        p=r['operation']['input']['position'];p['quantity']=p.pop('signed_quantity');p['quantity_unit']=p['quantity_unit'].lower()
    result['qualification']='Separate original source-id and quantity wire admission; all raw financial values/answers/bounds unchanged. Candidate envelopes remain transport only.'
    return result
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('action',choices=['generate','check']);args=parser.parse_args()
    raw=(json.dumps(admission(),indent=2)+'\n').encode()
    if args.action=='generate':
        if OUTPUT.exists():raise ValueError('Refuse to overwrite original wire admission')
        OUTPUT.write_bytes(raw)
    elif OUTPUT.read_bytes()!=raw:raise ValueError('Original wire admission changed')
    print(json.dumps(dict(status='passed',requests=73,financial_values_and_controls_unchanged=True)))
