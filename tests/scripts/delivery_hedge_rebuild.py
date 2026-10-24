"""Independent full all-member quote-rebuilt discount-curve controls."""
import argparse,copy,hashlib,json
from decimal import Decimal,localcontext
from delivery_hedge_reference import ROOT,HERE,D,serial,pack,key,settings,request,date,fraction,valuation
from delivery_hedge_ready import wire

TARGET=HERE/'delivery-hedge-rebuilt.json'
SOURCE='tests/fixtures/library/graph-hedge-api-02.json'

def generate():
    source=json.loads((ROOT/SOURCE).read_bytes());sample=next(c['request']['operation']['graph'] for c in source['cases'] if c['id']=='graph_hedge.rebuilt.irs')
    cases=[]
    with localcontext() as c:
        c.prec=60
        for control,quantity in [('stable',1),('switch',-3)]:
            resources,product=pack(control=control);curve=resources['curves'][0];g=copy.deepcopy(sample)
            rate=g['rates'][0];helper=rate['recipe']['helpers'][0];term=helper['terms']
            term['calendar']=dict(id='WEEKEND',version=2);term['curve']=copy.deepcopy(curve['key'])
            term['terms'].update(start_date='2026-10-07',maturity_date='2036-10-07',day_count='Act365Fixed')
            helper.update(id='delivery-full-deposit',pillar='2036-10-07',quote=dict(unit='DecimalRate',value=0.035),tolerance=1e-12)
            rate['recipe'].update(curve=copy.deepcopy(curve),pillars=['2036-10-07'],helpers=[helper],overnight_projection=None)
            rate['recipe']['curve'].update(calibration='Piecewise',source=dict(Helpers=['delivery-full-deposit']))
            rate['quote_versions']=[9];rate['futures_calendar_stamps']=[None]
            g.update(rates=[rate],credit=[],cash=[]);resources['curves']=[]
            q=D('0.035');T=fraction(date('2026-10-07'),date('2036-10-07'))
            z=(1+q*T).ln()/T
            oracle=copy.deepcopy(resources);oracle['curves']=[copy.deepcopy(curve)]
            oracle['curves'][0]['source']['Nodes']=[dict(pillar=dict(Tenor=0),quote=dict(unit='DecimalRate',value=z)),dict(pillar=dict(Tenor=T),quote=dict(unit='DecimalRate',value=z))]
            factor=key('discount',product['input']);factor['factor'].update(quote_type='deposit_rate',basis='market_quote_recalibrated',tenor='10Y')
            factor.update(coordinate=dict(kind='quote',curve=curve['key'],helper_id='delivery-full-deposit',quote_version=9),response='rebuild_dependents')
            policy=settings('.000005');h=D(str(policy['bump']));moves=[D(0),h,-h,h/2,-h/2]
            evaluated=[]
            zero=key('discount',product['input'])
            for move in moves:
                changed_z=(1+(q+move)*T).ln()/T
                e=valuation(product,oracle,zero,changed_z-z);e['delta']=move;evaluated.append(e)
            multiplier=D(product['input']['contract']['currency_per_price_point'])*quantity
            money=[v['fair_price_points']*multiplier for v in evaluated];winner=evaluated[0]['fair_deliverable']
            frozen=[next(a['forward_price_points'] for a in v['alternatives'] if a['instrument']==winner)*multiplier for v in evaluated]
            switch=any(e['fair_deliverable']!=winner for e in evaluated);selected=any(e['selected']!=evaluated[0]['selected'] for e in evaluated)
            ties=any(len(e['fair_ties'])>1 or len(e['selection_ties'])>1 for e in evaluated)
            expected=dict(evaluations=evaluated,base_pv=money[0],plus_pv=money[1],minus_pv=money[2],half_plus_pv=money[3],half_minus_pv=money[4],
                value=(money[1]-money[2])/(2*h)*D('.0001'),half_value=(money[3]-money[4])/h*D('.0001'),
                frozen_deliverable_sensitivity=(frozen[1]-frozen[2])/(2*h)*D('.0001'),frozen_deliverable_half_sensitivity=(frozen[3]-frozen[4])/h*D('.0001'),
                one_sided_plus=(money[1]-money[0])/h*D('.0001'),one_sided_minus=(money[0]-money[2])/h*D('.0001'),
                fair_deliverable_switch=switch,selection_switch=selected,ties_present=ties,ctd_stable=not(switch or selected or ties))
            p=policy['stability'];bound=D(str(p['absolute']))+D(str(p['relative']))*max(abs(expected['value']),abs(expected['half_value']))
            if abs(expected['value']-expected['half_value'])>bound: raise ValueError('Independent rebuilt full basket risk remains unresolved')
            r=request(resources,product,factor,policy,quantity);r['operation']['input']=g
            cases.append(dict(id='delivery.rebuilt.'+control,request=wire(r),expected=expected,expected_error=None))
    return dict(certified=False,qualification='Full original deposit helper and native-unit repricing rebuild all cash-member discount quotes, then same carry and CTD selection. Independent z=ln(1+qT)/T; all source units/revisions and every five-basket financial answer retained.',source_reads={SOURCE:hashlib.sha256((ROOT/SOURCE).read_bytes()).hexdigest()},cases=cases)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,indent=2,default=serial)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite independent rebuilt controls')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Full rebuilt reference differs')
    print(json.dumps(dict(status='passed',cases=len(v['cases']),certified=False)))
