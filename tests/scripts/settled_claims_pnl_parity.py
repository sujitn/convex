"""Actual separate native/C parity; native envelopes qualify transport only."""
import argparse,json
import cohort_pnl_admitted_parity as original
from settled_claims_pnl_census import source_cases
original.source_cases=source_cases
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--library');p.add_argument('--output');a=p.parse_args()
    print(json.dumps(original.parity(a.binary,a.library,a.output)))
