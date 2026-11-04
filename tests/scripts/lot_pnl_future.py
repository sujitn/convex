"""Original independent exact next earned coupon under full amended profiles."""
import argparse,hashlib,json
from decimal import Decimal as D,localcontext
from pathlib import Path
import causal_pnl_reference as c
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
SOURCE=FOLDER/'lot-pnl-reference.json';TARGET=FOLDER/'lot-pnl-future.json'
def generate():
    data=json.loads(SOURCE.read_bytes());b=data['cases'][0]['request']['operation']['input']['book']
    h=b['holdings'][b['steps'][1]['after']['holdings'][0]];terms=h['product']['product']['bond']['terms']
    with localcontext() as ctx:
        ctx.prec=60;amount=c.coupon(terms,'2026-10-07','2027-04-07')*D(h['lifecycle']['position']['value'])/D(terms['face_value'])
    return dict(certified=False,reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
        accrual_start='2026-10-07',accrual_end='2027-04-07',signed_currency_coupon=str(amount),original_holding=h)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Exact original future coupon control differs')
    print(json.dumps(dict(status='passed',cash_controls=1,certified=False)))
