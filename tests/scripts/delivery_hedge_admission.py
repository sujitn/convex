"""Immutable original-member/source/work and typed rejection controls."""
import argparse
import copy
from decimal import Decimal, localcontext
import json
from delivery_hedge_reference import HERE, ROOT, serial, pack, key, settings, request, graph, position, valuation

TARGET=HERE/'delivery-hedge-admission.json'

def generate():
    with localcontext() as c:
        c.prec=60
        resources,product=pack(control='stable')
        baseline=request(resources,product,key('discount',product['input']),settings())
        cases=[]
        for name in ['missing_member','duplicate_member','missing_cash_quote','missing_repo',
            'wrong_security_revision','wrong_future_revision','missing_haircut_rate','unused_haircut_rate',
            'future_known_quote','future_effective_repo','wrong_delivery_calendar','holiday_delivery',
            'funding_short_horizon','missing_discount','wrong_discount_source_revision','wrong_discount_role',
            'wrong_repo_source_revision','wrong_basis_source','nonmember_basis','wrong_factor_currency',
            'fractional_contracts','oversized_contracts','wrong_reference_size','member_work','zero_member_work',
            'ex_coupon','optional_member','unsupported_floating_member','unsupported_raw_futures_quote']:
            r=copy.deepcopy(baseline);a=r['operation']['action'];p=a['product'];f=p['input'];m=f['members'][0]
            error='invalid_input'
            if name=='missing_member': f['members'].pop()
            elif name=='duplicate_member': f['members'][1]=copy.deepcopy(m)
            elif name=='missing_cash_quote': del m['clean_price_per100']
            elif name=='missing_repo': del m['financing']
            elif name=='wrong_security_revision': m['stamp']['version']+=1
            elif name=='wrong_future_revision': f['basket']['contract']['version']+=1
            elif name=='missing_haircut_rate': m['own_cash']=None
            elif name=='unused_haircut_rate': m['financing']['haircut_fraction']='0'
            elif name=='future_known_quote': m['quote_stamp']['available_at']='2026-10-07T13:00:00Z'
            elif name=='future_effective_repo': m['financing']['stamp']['source_at']='2026-10-08T00:00:00Z'
            elif name=='wrong_delivery_calendar': f['calendar_stamp']['version']+=1
            elif name=='holiday_delivery': f['delivery']='2026-12-01'
            elif name=='funding_short_horizon': m['financing']['end']='2026-12-07'
            elif name=='missing_discount': p['discount']['curve']['version']='absent';error='missing_input'
            elif name=='wrong_discount_source_revision': p['discount']['stamp']['version']+=1
            elif name=='wrong_discount_role': r['inputs']['curves'][0]['role']=dict(kind='Government',market='SYNTH',security_type='nominal')
            elif name=='wrong_repo_source_revision': a['key']=key('repo_a',f);a['key']['coordinate']['version']+=1
            elif name=='wrong_basis_source': a['key']=key('basis_a',f);a['key']['coordinate']['input_id']='unrelated-cash-quote'
            elif name=='nonmember_basis': a['key']=key('basis_a',f);a['key']['factor']['target']['instrument_id']='NOT.A.MEMBER'
            elif name=='wrong_factor_currency': a['key']['factor']['currency']='EUR'
            elif name=='fractional_contracts': a['position']['quantity']='1.5'
            elif name=='oversized_contracts': a['position']['quantity']='1000000001'
            elif name=='wrong_reference_size': a['position']['reference_quantity']='2'
            elif name=='member_work': p['policy']['max_member_evaluations']=35
            elif name=='zero_member_work': p['policy']['max_member_evaluations']=0
            elif name=='ex_coupon': m['bond']['terms']['dates']['ex_coupon']=dict(CalendarDays=-3)
            elif name=='optional_member':
                m['bond']['terms']['exercise']=[dict(right='call',style=dict(style='european',date='2028-05-08'),
                    price_per_100='100',accrued='pay_accrued',notice_offset=dict(CalendarDays=0),settlement_lag=dict(CalendarDays=0),
                    settlement_adjustment='Unadjusted',protection_end=None)];error='unsupported'
            elif name=='unsupported_floating_member': m['bond']['terms']['coupon']=dict(kind='not-supported')
            elif name=='unsupported_raw_futures_quote':
                a['key']=dict(factor=dict(family='futures_price',target=dict(kind='instrument',instrument_id=f['contract']['instrument_id']),
                    currency='USD',tenor='spot',quote_type='futures_price',basis='raw_quote',convention_version=1,model_version='full-original-fixed-date-basket-v1'),
                    coordinate=dict(kind='raw',input_id=f['market']['stamp']['id'],version=f['market']['stamp']['version']),
                    native_unit='PricePer100',response='frozen_curves',unit='currency_per_price_point')
            cases.append(dict(id='delivery.invalid.'+name,request=r,expected_error=error))
        for axis in ['discount','repo_a','basis_a']:
            rsrc,p=pack(control='switch');factor=key(axis,p['input']);base=valuation(p,rsrc,factor,Decimal(0))
            for move in ['0.001','-0.001'] if axis!='basis_a' else ['0.05','-0.05']:
                r=request(rsrc,p,factor,settings(),-3);a=r['operation']['action'];a.update(kind='product_scenario',delta=float(move));del a['settings']
                cases.append(dict(id='delivery.scenario.'+axis+'.'+move,request=r,expected_error=None,
                    expected=dict(baseline=base,shocked=valuation(p,rsrc,factor,Decimal(move)))))
        rsrc,p=pack(control='stable');p['input']['members'].reverse();p['input']['basket']['bonds'].reverse()
        factor=key('discount',p['input']);r=request(rsrc,p,factor,settings())
        from delivery_hedge_reference import answer
        cases.append(dict(id='delivery.reordered.full_basket',request=r,expected_error=None,expected=answer(p,rsrc,factor,settings())))
    return dict(certified=False,qualification='Missing/unsupported/source/work controls and full separately computed raw signed scenarios; original member economics never omitted.',cases=cases)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,indent=2,default=serial)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite independent original admission controls')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Original admission control differs')
    print(json.dumps(dict(status='passed',requests=len(v['cases']),certified=False)))
