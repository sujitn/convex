"""New independent admission revision; preserve the first declaration unchanged.

The first government examples used a short final stub whose payment coincides
with the previous adjusted coupon. Native construction correctly rejects it.
This separate declaration supplies contractual October-7 maturities and retains
the actual following cash date as the helper horizon. All bounds stay unchanged.
"""
import argparse
import json
import product_parametric_reference as original
import cash_calibration_reference as cash

DEST=original.ROOT/'tests/fixtures/curves/product-parametric-admitted.json'

def build():
    fixture=original.build()
    for case in fixture['cases']:
        if '.government.' not in case['id']:continue
        recipe=case['inputs']['calibration']['cash'][0]['recipe']
        for helper,expected in zip(recipe['helpers'],case['expected']):
            h=helper['input'];terms=h['terms'];end=terms['schedule']['end_date']
            if end[5:] not in ['10-08','10-09']:continue
            maturity=end[:8]+'07';terms['schedule']['end_date']=maturity
            for rows in terms['profiles'].values():
                for entry in rows:
                    if entry['end']==end:entry['end']=maturity
            flows=cash.cash(terms,original.REF,[])
            price=cash.dirty(flows,original.REF,lambda on:original.df(case['model'],on))
            h['quote']['value']=float(price);expected['native_quote']=h['quote']
    fixture['admission_revision']='Actual contractual maturity differs from adjusted final cash date. First frozen declaration and failed native checks remain historical controls.'
    return fixture

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--check',action='store_true');args=parser.parse_args()
    content=json.dumps(build(),indent=2)+'\n'
    if args.check:
        if DEST.read_text()!=content:raise SystemExit('Frozen admitted control bytes differ')
        print('12 original product graphs /60 Decimal controls and exact contractual/cash horizons passed')
    else:
        if DEST.exists():raise SystemExit('Refusing to overwrite independent admission revision')
        DEST.write_text(content)
