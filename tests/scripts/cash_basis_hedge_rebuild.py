"""Separate predeclared full-helper rebuild, opaque-version and recovery boundaries.

Original inputs/answers remain unchanged. Rebuilt cash prices come from the
original observed dirty price or the same explicit full-cash ICMA quotation;
no fitted node or captured native output is used as a financial expectation.
"""
import argparse
from copy import deepcopy as clone
from decimal import Decimal as D
import hashlib
import json
from pathlib import Path
from cash_basis_hedge_reference import FOLDER, ROOT, project, value, quote, MODEL, MULTIPLIER, quote_price

SOURCES=['tests/fixtures/library/cash-basis-hedge-api.json','tests/fixtures/library/cash-basis-hedge-reference.json','tests/scripts/cash_basis_hedge_reference.py']

def generate():
    raw=json.loads((ROOT/SOURCES[0]).read_bytes())['cases'];refs={c['id']:c['expected'] for c in json.loads((ROOT/SOURCES[1]).read_bytes())['cases']};cases=[];answers=[]
    def add(id,r,a=None,error=None):
        cases.append(dict(id=id,request=r,expected_error=error))
        if a is not None:answers.append(dict(id=id,expected=a))
    for label in ['dirty','icma']:
        src=next(c for c in raw if c['id']=='cash_basis_hedge.positive.credit.'+label);r=clone(src['request']);a=clone(refs[src['id']]);op=r['operation'];h=op['security']['helper'];basiscurve=r['inputs']['curves'].pop();basiscurve['calibration']='Piecewise';basiscurve['source']=dict(Helpers=[h['id']]);basiscurve['dependencies']=[op['input']['model']['discount'],op['input']['model']['survival']]
        # Use a complete comparison CDS within the actual fitted basis horizon.
        if label=='dirty':op['input']['credit']['cds']['premium_schedule']['end_date']='2027-09-20'
        op['input']['credit']['basis']['dated_spreads'][-1][0]=h['pillar']
        p=a['cash'];p['accrued_per100']=D(p['accrued_per100']);p['accrued']=D(p['accrued'])
        native,implied=quote(h,p,D(a['valuation']['dirty_per100']));h['quote']['value']=float(native)
        # Literal observed native-unit repricing is an independent original quote.
        if label=='icma':
            y=D(str(h['quote']['value']))/MULTIPLIER[h['quote']['unit']]
            observed_icma=quote_price(h,p['cashflows'],p['accrued_per100'],y)
            observed_dirty=D(observed_icma['dirty_per100']);a['observed']=quote(h,p)
            a['model_icma']=observed_icma
        else:observed_dirty=D(str(h['quote']['value']));a['observed']=quote(h,p)
        pv=observed_dirty*D(h['terms']['face_value'])/100
        op['graph']['cash']=[dict(recipe=dict(curve=basiscurve,pillars=[h['pillar']],helpers=[dict(kind='Bond',input=clone(h))],bootstrap=dict(lower_parameter=-.05,upper_parameter=.1,max_iterations=128),basis_model=clone(op['input']['model']),overlap='Reject'),calendars=[dict(kind='bond',pins=clone(op['security']['calendars']))])]
        fs=op['input']['hedge']['factors'];fs[0]['key']['response']='rebuild_dependents';a['factors'][0]=dict(prices=[str(pv)]*5,value='0')
        qkey=clone(fs[2]);qkey['key']['factor'].update(family='interest_rate' if label=='icma' else 'cash_price',quote_type='yield' if label=='icma' else 'cash_dirty_price',basis='market_quote_recalibrated',tenor='actual-full-cash',convention_version=h['quote_kind']['input']['sources']['quotation_policy']['version'] if label=='icma' else 1)
        qkey['key'].update(coordinate=dict(kind='quote',curve=clone(op['input']['basis']),helper_id=h['id'],quote_version=h['quote_version']),response='rebuild_dependents',native_unit=h['quote']['unit'],unit='currency_per_bp' if label=='icma' else 'currency_per_price_point')
        fs.append(qkey);op['input']['hedge']['max_factor_evaluations']=30
        amounts=[]
        for move in [D(0),D('.0001'),D('-.0001'),D('.00005'),D('-.00005')]:
            if label=='icma':dirty=D(quote_price(h,p['cashflows'],p['accrued_per100'],D(str(h['quote']['value']))/MULTIPLIER[h['quote']['unit']]+move)['dirty_per100'])
            else:dirty=observed_dirty+move
            amounts.append(dirty*D(h['terms']['face_value'])/100)
        scale=D('.0001') if label=='icma' else D(1)
        a['factors'].append(dict(prices=list(map(str,amounts)),value=str((amounts[1]-amounts[2])/(2*D('.0001'))*scale)))
        a['model_quote']=str(D(str(h['quote']['value'])))
        # Source model PV differs by only the quote's explicit IEEE ingress rounding;
        # unchanged native residual and full model-price bounds are used.
        add('cash_basis_hedge.rebuilt.'+label,r,a)
        negative=clone(r);negative['operation']['input']['hedge']['factors'][2]['key']['response']='rebuild_dependents'
        add('cash_basis_hedge.invalid.calibrated_basis_rebuild.'+label,negative,error='invalid_input')
    src=next(c for c in raw if c['id']=='cash_basis_hedge.positive.credit.clean')
    r=clone(src['request']);r['inputs']['curves'][-1]['key']['version']='explicit.opaque.version';r['operation']['input']['basis']['version']='explicit.opaque.version';r['operation']['input']['hedge']['factors'][2]['key']['coordinate']['curve']['version']='explicit.opaque.version'
    add('cash_basis_hedge.opaque_curve_version',r,clone(refs[src['id']]))
    for recovery in [D(0),D(1)]:
        r=clone(src['request']);op=r['operation'];rec=float(recovery);op['input']['model']['recovery']=rec;r['inputs']['curves'][2]['role']['recovery']=rec
        for ref in [op['input']['credit']['mapping']['cds_reference'],op['input']['credit']['cds']['reference']]:ref['recovery_fraction']=rec
        op['input']['credit']['basis']['recovery_fraction']=rec
        del op['input']['hedge']['factors'][3];op['input']['hedge']['max_factor_evaluations']=20
        a=clone(refs[src['id']]);p=a['cash'];p['accrued_per100']=D(p['accrued_per100']);p['accrued']=D(p['accrued']);h=op['security']['helper'];a['valuation']=value(h,p,D('.03'),D('.02'),D('.0025'),recovery);a['model_quote']=str(quote(h,p,D(a['valuation']['dirty_per100']))[0]);a['factors']=[]
        for kind in ['zero','hazard','basis','unrelated']:
            values=[]
            for move in [D(0),D('.0001'),D('-.0001'),D('.00005'),D('-.00005')]:
                values.append(D(value(h,p,D('.03')+(move if kind=='zero' else 0),D('.02')+(move if kind=='hazard' else 0),D('.0025')+(move if kind=='basis' else 0),recovery)['present_value']))
            a['factors'].append(dict(prices=list(map(str,values)),value=str((values[1]-values[2])/(2*D('.0001'))*(1 if kind=='hazard' else D('.0001')))))
        add('cash_basis_hedge.recovery_boundary.'+str(recovery),r,a)
    return dict(certified=False,cases=cases),dict(certified=False,source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},qualification='Separate full quote-rebuild and boundary controls; original raw controls unchanged. No native observed output is a financial oracle.',cases=answers)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    for name,doc in zip(['cash-basis-hedge-rebuild.json','cash-basis-hedge-rebuild-reference.json'],generate()):
        p=FOLDER/name;b=(json.dumps(doc,indent=2,default=str)+'\n').encode()
        if args.write:
            if p.exists():raise ValueError('Refusing to overwrite controls')
            p.write_bytes(b)
        elif p.read_bytes()!=b:raise ValueError('Independent original controls differ')
        print(json.dumps(dict(status='passed',path=name,cases=len(doc['cases']),certified=False)))
