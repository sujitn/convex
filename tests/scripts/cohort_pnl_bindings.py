"""Separate exact full market/source binding and original cash-kind ownership.

Preserve all prior inputs, references and failed checks. New original acquisition
products bind to each actual full point's discount revision. Synthetic stamp IDs
use the original strict ASCII representation. Actual termination-agreement cash
is not an embedded original swap coupon: remove no such agreement from exposure.
Both financial and native decompositions use exact original coupon kinds; original
guards/bounds remain. JSON node-budget failure stays resource_limit. No candidate
financial output is read, fitted, clamped or made a financial reference.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as s
import causal_pnl_ready as binding
import causal_pnl_wire as wire
from delivery_hedge_ready import wire as identifiers
SOURCE=r.TARGET.with_name('cohort-pnl-contracts.json');TARGET=SOURCE.with_name('cohort-pnl-bindings.json')
def explain(data,spec,book):
    for trade in data['trades']:
        for reporting in [False,True]:
            levels=[]
            for mark in trade['marks']:
                cash=D(mark['recorded'])-D(trade['marks'][0]['recorded'])+(D(mark['consideration']) if mark['basis']=='CurrencyPresentValue' else D(0))+D(mark['fees']);adj=cash if mark['basis']=='CurrencyPresentValue' else D(mark['fees'])-D(mark['consideration'])+D(mark['terminal_quote'] or 0);rate=D(mark['fx']) if reporting else D(1);values={k:D(v) for k,v in mark['reporting' if reporting else 'local'].items()};levels.append(dict(marking=values,cash=cash*rate,current_model_pnl=values['current']+adj*rate))
            trade['reporting' if reporting else 'local']=r.c.explain(book,levels)
    for key,reporting in [('local_totals',False),('reporting_totals',True)]:
        for total in data[key]:
            selected=[t for j,t in enumerate(data['trades']) if t['marks'][0]['basis']==total['basis'] and (reporting or book['before']['point']['positions'][j]['current']['currency']==total['currency'])]
            total['attribution']=r.c.explain(book,[r.c.sum_rows([t['reporting' if reporting else 'local']['levels'][i] for t in selected]) for i in range(len(r.sets(book)))])
def generate():
    data=json.loads(SOURCE.read_bytes())
    with localcontext() as context:
        context.prec=60
        for case in data['cases']:
            identifiers(case['request']);spec=case['request']['operation']['input'];book=s.expand(case['request'])
            for info in r.sets(book):
                for j,h in enumerate(info['holdings']):
                    if h['id']!='cohort.new.cash':continue
                    original=h['product']['product']['method']['curve'];actual=next(x['key'] for x in info['inputs']['curves'] if x['key']['id']==original['id']);h['product']['product']['method']['curve']=copy.deepcopy(actual);info['point']['positions'][j]['cash_discount']=copy.deepcopy(actual)
            req=r.c.request(book['before'],[],[]);req['operation']['input']=book;encoded=wire.normalize(binding.encode(req));spec['book']=encoded['operation']['input']
            if case['id']=='cohort.invalid.entitlement_budget':case['expected_error']='resource_limit'
            if case['expected_error'] is not None:continue
            expected=case['expected']
            for trade in expected['trades']:
                for mark in trade['marks']:
                    removed=D(0)
                    for cash in mark['claims']:
                        if cash['key']['kind']=='termination':removed+=D(cash['removed']);cash['removed']=D(0)
                    for kind in ['current','projected']:
                        mark['local'][kind]=D(mark['local'][kind])+removed;mark['reporting'][kind]=D(mark['reporting'][kind])+removed*D(mark['fx'])
            explain(expected,spec,book)
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original complete binding/kind declaration differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
