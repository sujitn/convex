#!/usr/bin/env python3
"""Independent Decimal60 dated full-cash price/inverse references; no Convex output."""
from decimal import Decimal as D, getcontext
from pathlib import Path
import argparse, hashlib, json
import cash_calibration_reference as cash

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT/'tests/fixtures/workflow/standalone.json'
SOURCES = ['tests/fixtures/risk/quote-risk.json', 'tests/scripts/cash_calibration_reference.py']

def build():
    source = json.loads((ROOT/SOURCES[0]).read_text())
    helper = source['helper']; settlement = helper['settlement']
    terms = cash.cash(helper['terms'], settlement, set(source['holidays']))
    accrued = terms['accrued']/terms['face']*100
    cases = []
    def solve(price, target, lo, hi):
        for _ in range(240):
            middle = (lo+hi)/2
            if price(middle) > target: lo = middle
            else: hi = middle
        return (lo+hi)/2
    for value in ['-0.02','0.031','0.1']:
        rate = D(value)
        price = lambda y: cash.yield_price(terms, settlement, y)
        dirty = price(rate)
        inverse = solve(price, dirty, D('-0.9'), D('1'))
        cases.append(dict(kind='yield',rate=str(rate),dirty=str(dirty),clean=str(dirty-accrued),accrued=str(accrued),inverse=str(inverse)))
    for value in ['-0.003','0','0.01234567']:
        spread = D(value); base = D('0.035')
        price = lambda s: sum((amount*(-(base+s)*cash.span(settlement, date)).exp() for date,amount in terms['flows']),D(0))/terms['face']*100
        dirty = price(spread)
        inverse = solve(price, dirty, D('-0.1'), D('0.2'))
        cases.append(dict(kind='spread',reference_continuous_zero=str(base),rate=str(spread),dirty=str(dirty),clean=str(dirty-accrued),accrued=str(accrued),inverse=str(inverse)))
    return (json.dumps(dict(schema_version=1,certified=False,qualification='Independent synthetic engineering reference; no human quantitative or release certificate.',method='Decimal60 manual actual coupon and principal cash/profile/settlement, semiannual ACT365F yield and continuous ACT365F Z spread,240 fixed bisections with no candidate reads.',tolerances=dict(per100_absolute='1e-9',inverse_rate_absolute='1e-10'),source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},helper=helper,holidays=source['holidays'],cases=cases),indent=2)+'\n').encode()

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('mode',choices=['generate','check']);mode=parser.parse_args().mode
    data=build()
    if mode=='generate':DEST.parent.mkdir(parents=True,exist_ok=True);DEST.write_bytes(data)
    elif DEST.read_bytes()!=data:raise ValueError('Independent standalone reference bytes differ')
    print(json.dumps(dict(status='passed',mode=mode,cases=6,sha256=hashlib.sha256(data).hexdigest(),certified=False)))

if __name__=='__main__':main()
