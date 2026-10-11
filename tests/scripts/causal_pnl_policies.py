"""Exact original swap fixing policies on unchanged complete economic controls.

The original fixtures remain immutable, including the rejected mismatched
context. Bind the two full IRS/OIS contexts to the already supplied original
contract policies. All observations used by the independent full dated cash
controls are already known; no financial answer, term or tolerance changes.
"""
import argparse,hashlib,json
import causal_pnl_reference as c
SOURCE=c.FOLDER/'causal-pnl-fixing-cutoffs.json'
TARGET=SOURCE.with_name('causal-pnl-policies.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for case in data['cases']:
        if case['id'] not in ['causal.time.irs_full_cash','causal.time.ois_full_cash']:continue
        for h in case['request']['operation']['input']['holdings']:
            p=h['product']['product'];p['context']['fixings'].update(p['terms']['floating']['fixing_policy'])
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Separate exact original IRS/OIS contractual fixing-policy binding on unchanged complete supplied terms and original dated independent cash/P&L controls and bounds. Original rejected context remains immutable upstream. No candidate output read.')
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original policy binding differs')
    print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
