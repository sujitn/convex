"""Independent original IRS delay-then-adjust date rules at unchanged bounds.

Retain the original hand-entered Apr03 financial trial. Derive complete coupon
due dates independently from SAME original calendars/date rules: unadjusted
Mar31 plus two business days, THEN Following, is Apr02. Recompute complete dated
PV/claim/P&L controls without candidate output; no original contract, contractual
amount, financial owner, earlier fixture or tolerance is modified.
"""
import argparse, copy, hashlib, json, datetime as dt
from decimal import Decimal as D, localcontext
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_ready as binding
import causal_pnl_wire as wire
SOURCE=c.FOLDER/'causal-pnl-policies.json'
TARGET=SOURCE.with_name('causal-pnl-payment-dates.json')

def generate():
    data=json.loads(SOURCE.read_bytes())
    case=next(x for x in data['cases'] if x['id']=='causal.time.irs_full_cash')
    req=wire.expand(case['request']);inp=req['operation']['input'];terms=inp['before']['holdings'][0]['product']['product']['terms']
    original=a.swap_flows
    def flows(t):
        result=original(t)
        for name,rows in result.items():
            leg=t[name]['leg'];calendar=next(x for x in inp['before']['inputs']['calendars'] if x['pin']==leg['calendars']['payment'])
            holidays=set(calendar['holidays']);weekends=set(calendar['weekend_days'])
            def business(d):return d.weekday() not in weekends and str(d) not in holidays
            for row in rows:
                end=c.day(row['end']);due=end;delay=leg['dates']['payment_delay'];n=delay['BusinessDays']
                while n:
                    due+=dt.timedelta(days=1)
                    if business(due):n-=1
                if leg['dates']['payment_adjustment']!='Following':raise ValueError('Independent original control requires its actual Following policy')
                while not business(due):due+=dt.timedelta(days=1)
                row['payment']=str(due)
        return result
    original_native=a.native
    def native(info,h):
        p=h['product']['product'];on=info['point']['valuation_date'];z=c.curve_zero(info['inputs'],p['terms']['collateral']['discount_curve']['id']);out={'fixed':[],'floating':[]};pv=D(0)
        for leg,rows in flows(p['terms']).items():
            for row in rows:
                if row['payment']<=on:continue
                row=copy.deepcopy(row);value=row['amount']*(-z*D((c.day(row['payment'])-c.day(on)).days)/365).exp();row['pv']=value;out[leg].append(row);pv+=value
        return dict(kind='swap',pv=pv,full=out)
    with localcontext() as context:
        context.prec=60;opening=c.ClaimSet([[]]);opening._events=[0];a.swap_flows=flows;a.native=native
        try:expected=a.financial(req,opening)
        finally:a.swap_flows=original;a.native=original_native
    case['expected']=expected
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Separate independently date-rule-derived original IRS delay-then-adjust due dates and complete PV/cash/claims/P&L at unchanged contractual amounts and original bounds. Raw Apr03 hand-entered financial trial and Native41/42 failures remain immutable upstream. No candidate output used.')
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();raw=(json.dumps(generate(),indent=2,default=c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independently derived original cash dates differ')
    print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
