"""Separate scaled financial limits; raw unresolved broad-limit solve stays.

Explicit factor scales/limits retain the SAME original actual columns, known
trades/targets, side costs, lots and every numerical tolerance/work policy.
"""
from copy import deepcopy
import argparse, hashlib, json
from pathlib import Path
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');a=parser.parse_args()
    source=FOLDER/'graph-hedge-actual-cash-problems.json';raw=source.read_bytes();value=deepcopy(json.loads(raw))
    for case in value['cases']:
        for i,f in enumerate(case['factors']):
            f['lower']=-1000000.0 if i==3 else -1000.0;f['upper']=-f['lower']
            if i in [1,2]:f['scale']=100.0
    value['application_limits_source_sha256']=hashlib.sha256(raw).hexdigest()
    value['qualification']+=' Separate scaled financial exposure limits; raw broad-limit numerical_failure is not promoted. Original numerical policy, actual prices/trades/targets/costs/lots stay.'
    data=(json.dumps(value,indent=2)+'\n').encode();dest=FOLDER/'graph-hedge-limits.json'
    if a.write:
        if dest.exists():raise ValueError('Refusing to replace new financial limits')
        dest.write_bytes(data)
    elif dest.read_bytes()!=data:raise ValueError('Financial limits differ')
    print(json.dumps(dict(status='passed',cases=2,original_numerical_policy_unchanged=True,certified=False)))
