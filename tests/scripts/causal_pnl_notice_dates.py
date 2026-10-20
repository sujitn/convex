"""Independent signed native notice offset on the full original optional bond.

Preserve the failed original financial trial. The retained Gaussian helper
accepts a positive notice lead; the actual native contract accepts a signed
date offset. Adapt ONLY the independent helper's argument, leaving full legal
terms untouched, then independently recompute the complete native-model/cash/
claims/P&L controls. No candidate output or comparison bound is read or changed.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_ready as binding
import causal_pnl_wire as wire
SOURCE=c.FOLDER/'causal-pnl-payment-dates.json'
TARGET=SOURCE.with_name('causal-pnl-notice-dates.json')

def generate():
    data=json.loads(SOURCE.read_bytes());case=next(x for x in data['cases'] if x['id']=='causal.event.full_notice_exercise_claim')
    req=wire.expand(case['request']);inp=req['operation']['input'];original=a.native
    def native(info,h):
        p=h['product']['product'];input_=copy.deepcopy(p['input'])
        for term in input_['bond']['terms']['exercise']:
            # The actual last-notice date is exercise + signed offset. The
            # original closed Gaussian helper takes an unsigned lead instead.
            term['notice_offset']['CalendarDays']=-term['notice_offset']['CalendarDays']
        discount=c.curve_zero(info['inputs'],input_['discount']['id']);projection=c.curve_zero(info['inputs'],input_['projection']['id']) if input_['projection'] else D('.05')
        model=input_['model']['parameters'];value=c.op.financial_price(input_,info['inputs'],discount=float(discount),projection=float(projection),oas=p['oas_decimal'],a=model['mean_reversion'],sigma=model['volatility'])
        return dict(kind='optional',pv=D(str(value['pv'])),full=value)
    opening_rows=[]
    for row in case['expected']['trades'][0]['marks'][0]['claims']:
        opening_rows.append(dict(kind=row['key']['kind'],effective=row['key']['effective_date'],ordinal=row['key']['ordinal'],due=row['due'],amount=D(row['amount']),recorded=D(row['recorded'])))
    with localcontext() as context:
        context.prec=60;opening=c.ClaimSet([opening_rows]);opening._events=[len(inp['before']['holdings'][0]['lifecycle']['events'])];a.native=native
        try:case['expected']=a.financial(req,opening)
        finally:a.native=original
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Separate original signed notice-date semantic adaptation for the independent full Gaussian helper, preserving complete legal terms. Recomputed actual full prices, original exercise cash and P&L at unchanged bounds; original April-notice trial and Native48 failure retained. No candidate output read.')
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();raw=(json.dumps(generate(),indent=2,default=c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent original signed notice dates differ')
    print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
