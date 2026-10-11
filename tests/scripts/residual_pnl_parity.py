"""Actual separate C parity; retained native envelopes prove transport only."""
import argparse,json
import cohort_pnl_admitted_parity as original
from residual_pnl_census import source_cases
def parity(binary,library=None,output=None):
    original.source_cases=source_cases
    return original.parity(binary,library,output)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--library');p.add_argument('--output');a=p.parse_args()
    print(json.dumps(parity(a.binary,a.library,a.output)))
