"""Independent Decimal60 analytic/finite-lattice controls; never reads candidate output."""
from copy import deepcopy
from decimal import Decimal, getcontext
import argparse
import json
from pathlib import Path
getcontext().prec=60
D=Decimal
ROOT=Path(__file__).resolve().parents[2]
FOLDER=ROOT/'tests/fixtures/library'
CREDIT={'entity_id':'ACME','seniority':'SeniorUnsecured','document':{'id':'LEGAL.ACME','version':7},
        'restructuring_clause':{'id':'RESTRUCTURING.ACME','version':8},'recovery_fraction':0.4}
SETTINGS={'policy_version':9,'rank_relative_tolerance':1e-10,'max_condition':10000.0,
          'solver_tolerance':1e-10,'factor_tolerance':1e-9,'cost_tolerance':1e-9,
          'cost_scale':1.0,'cost_weight':0.0,'max_work':100000,'svd_iterations':1000}
RISK_SETTINGS={'bump':0.0001,'stability':{'absolute':1e-8,'relative':0.0},'policy_version':3}

def factor(i,credit=False,basis=False):
    name=['USD.DISC','USD.CREDIT','USD.PROJ','USD.BASIS'][i]
    q='ResidualBasis' if basis else 'CdsSpread' if credit else 'ZeroRate'
    # Canonical enums use their original snake-case wire spellings.
    key={'factor':{'family':'credit_spread' if credit else 'interest_rate',
         'target':{'kind':'curve','curve_id':name},'currency':'USD','tenor':'parallel',
         'quote_type':{'ResidualBasis':'residual_basis','CdsSpread':'cds_spread','ZeroRate':'zero_rate'}[q],
         'basis':'market_quote_recalibrated' if credit and not basis else 'zero_node',
         'convention_version':7,'model_version':'MODEL.RISK.v1'},
         'coordinate':({'kind':'basis' if basis else 'zero','curve':{'id':name,'version':i+3},'node':None}
                       if not credit or basis else {'kind':'quote','curve':{'id':name,'version':i+3},'helper_id':'CDS.ACME.5Y','quote_version':5}),
         'native_unit':'DecimalRate','response':'rebuild_dependents' if credit and not basis else 'frozen_curves',
         'unit':'currency_per_bp'}
    return {'key':key,'risk_settings':deepcopy(RISK_SETTINGS),'credit':deepcopy(CREDIT) if credit else None,
            'scale':1.0,'weight':1.0,'target':0.0,'lower':-1000.0,'upper':1000.0}

FACTORS=[factor(0),factor(1,True),factor(2),factor(3,True,True)]

def vector(identity,kind,values,factors,reference='1000',currency='USD',fx=None):
    position={'quantity':reference,'reference_quantity':reference,'quantity_unit':'contracts' if kind=='futures' else 'notional','currency':currency}
    sensitivities=[]
    for f,value in zip(factors,values):
        v=float(value);b=RISK_SETTINGS['bump']
        sensitivities.append({'key':deepcopy(f['key']),'position':deepcopy(position),'snapshot_id':'SNAP.1',
          'settings':deepcopy(RISK_SETTINGS),'method':'central','base_pv':0.0,'plus_pv':v,'minus_pv':-v,
          'half_plus_pv':v/2,'half_minus_pv':-v/2,'value':(v-(-v))/(2*b)*0.0001,'half_value':(v/2-(-v/2))/b*0.0001})
    scope=[deepcopy(CREDIT)] if kind=='cds' or any(f['credit'] and float(v)!=0 for f,v in zip(factors,values)) else []
    return {'id':identity,'product':kind,'instrument':{'id':'INSTRUMENT.'+identity,'version':2},
          'economics':{'id':'FULL.TERMS.'+identity,'version':3},'calculation':{'id':'FULL.MODEL.'+identity,'version':4},
          'entity':'DESK.A','settlement':'2026-01-05','credit_scope':scope,'reference_position':position,
          'sensitivities':sensitivities,'fx':fx}

def candidate(v):
    ref=D(v['reference_position']['reference_quantity']);lim=ref*10
    return {'risk':v,'eligibility':{'eligible':True,'decision':{'id':'ELIGIBILITY.'+v['id'],'version':5},'reason':'Supplied fictional engineering eligibility; no order authority'},
          'trade_lower':str(-lim),'trade_upper':str(lim),'holding_lower':str(-lim*2),'holding_upper':str(lim*2),
          'buy_available':str(lim),'sell_available':str(lim),'lot':'1' if v['product']=='futures' else None,
          'costs':{'currency':v['reference_position']['currency'],'buy_per_reference':'0','sell_per_reference':'0','observation':{'id':'ACTUAL.SUPPLIED.COSTS.'+v['id'],'version':6}}}

def problem(before,columns,kinds=None,factors=None):
    factors=deepcopy(factors or FACTORS[:len(before)])
    if kinds is None:kinds=['cash_bond','cds','irs','futures'][:len(columns)]
    origin=vector('ORIGIN','cash_bond',before,factors)
    return {'snapshot_id':'SNAP.1','entity':'DESK.A','settlement':'2026-01-05','reporting_currency':'USD',
       'position_state':{'id':'CURRENT.AND.PENDING','version':11},'expected_holdings':['ORIGIN'],
       'holdings':[{'risk':origin,'current_quantity':'1000','pending_quantity':'0'}],
       'factors':factors,'candidates':[candidate(vector('HEDGE.'+str(i),kind,col,factors,'1' if kind=='futures' else '1000')) for i,(kind,col) in enumerate(zip(kinds,columns))],
       'max_cost':1000.0,'settings':deepcopy(SETTINGS)}

def calculate(p,multiples,lot_search=False):
    """Original economic arithmetic, independent of any Rust solver/results."""
    factors=p['factors'];current=[D(0)]*len(factors);pending=current[:]
    for h in p['holdings']:
        ref=D(h['risk']['reference_position']['reference_quantity']);fx=D(h['risk']['fx']['rate']) if h['risk']['fx'] else D(1)
        for i,s in enumerate(h['risk']['sensitivities']):
            # Original central sensitivities are supplied; use their exact wire values.
            value=D(str(s['value']))*fx
            current[i]+=value*D(h['current_quantity'])/ref
            pending[i]+=value*D(h['pending_quantity'])/ref
    before=[a+b for a,b in zip(current,pending)]
    def evaluate(xs):
        after=before[:];costs=[];quantities=[];holdings=[]
        for c,x in zip(p['candidates'],xs):
            fx=D(c['risk']['fx']['rate']) if c['risk']['fx'] else D(1)
            ref=D(c['risk']['reference_position']['reference_quantity']);q=ref*x
            quantities.append(q)
            held=sum((D(h['current_quantity'])+D(h['pending_quantity']) for h in p['holdings'] if h['risk']['id']==c['risk']['id']),D(0));holdings.append(held+q)
            if not (D(c['trade_lower'])<=q<=D(c['trade_upper']) and -D(c['sell_available'])<=q<=D(c['buy_available'])
                    and D(c['holding_lower'])<=held+q<=D(c['holding_upper'])):return None
            cost=(x*D(c['costs']['buy_per_reference']) if x>=0 else -x*D(c['costs']['sell_per_reference']))*fx;costs.append(cost)
            for i,s in enumerate(c['risk']['sensitivities']):after[i]+=D(str(s['value']))*fx*x
        total=sum(costs,D(0))
        if total>D(str(p['max_cost'])):return None
        if any(not D(str(f['lower']))<=a<=D(str(f['upper'])) for f,a in zip(factors,after)):return None
        residual=[a-D(str(f['target'])) for a,f in zip(after,factors)]
        tracking=sum((D('0.5')*D(str(f['weight']))*(r/D(str(f['scale'])))**2 for f,r in zip(factors,residual)),D(0))
        objective=tracking+D(str(p['settings']['cost_weight']))*total/D(str(p['settings']['cost_scale']))
        return {'quantities':[str(q) for q in quantities],'reference_multiples':[str(x) for x in xs],
          'final_holdings':[str(q) for q in holdings],'after':[str(v) for v in after],'residual':[str(v) for v in residual],
          'candidate_costs':[str(v) for v in costs],'total_cost':str(total),'tracking_objective':str(tracking),'objective':str(objective)}
    continuous=evaluate([D(str(v)) for v in multiples])
    proposal=continuous
    if lot_search:
        # Controls use only one lot axis and diagonal/orthogonal separation.
        # Enumerate the ORIGINAL exact native lattice, independently of Rust code.
        indices=[i for i,c in enumerate(p['candidates']) if c['lot']]
        assert len(indices)==1
        j=indices[0];c=p['candidates'][j];lot=D(c['lot']);ref=D(c['risk']['reference_position']['reference_quantity'])
        lo=int((D(c['trade_lower'])/lot).to_integral_value(rounding='ROUND_CEILING'))
        hi=int((D(c['trade_upper'])/lot).to_integral_value(rounding='ROUND_FLOOR'))
        proposal=None
        for n in range(lo,hi+1):
            xs=[D(str(v)) for v in multiples];xs[j]=D(n)*lot/ref
            value=evaluate(xs)
            if value and (proposal is None or D(value['objective'])<D(proposal['objective'])):proposal=value
    return {'current':[str(v) for v in current],'pending':[str(v) for v in pending],'before':[str(v) for v in before],
            'continuous':continuous,'proposal':proposal}

def generate():
    cases=[];refs=[]
    def add(name,p,x=None,status='feasible',action='solve',error=None,lots=False):
        case={'id':'hedge.'+name,'expected_error':error,'expected_status':status if action=='solve' and error is None else None,
             'request':{'schema_version':1,'inputs':{'calendars':[],'helper_calendars':[],'curves':[],'calibration':None,'fixings':[],'publications':[]},
                        'operation':{'kind':'hedge','problem':deepcopy(p),'action':{'kind':action}}}}
        cases.append(case)
        if x is not None:
            expected=calculate(p,x,lots)
            if status=='feasible' and action=='solve':assert expected['continuous'] and expected['proposal'],name
            refs.append({'id':case['id'],'expected':expected,'absolute':1e-8,'quantity_absolute':1e-7,'relative':0.0})
        return case
    eye=[[1 if i==j else 0 for i in range(4)] for j in range(4)]
    base=problem([8,-6,4,2],eye)
    add('mixed',base,[-8,6,-4,-2],lots=True)
    add('aggregate',base,[-8,6,-4,-2],action='aggregate')
    for name,b in [('signed',[-4,3,-2,-1]),('zero',[0,0,0,0])]:
        p=problem(b,eye);add(name,p,[-D(v) for v in b],lots=True)
    p=deepcopy(base);p['holdings'][0]['current_quantity']='-500';p['holdings'][0]['pending_quantity']='250'
    add('pending',p,[2,-D('1.5'),1,D('.5')],lots=True)
    p=deepcopy(base);p['settings']['cost_weight']=1.0
    for c in p['candidates']:c['costs']['buy_per_reference']='0.5';c['costs']['sell_per_reference']='1'
    add('side_costs',p,[-7,D('5.5'),-3,-1],lots=True)
    p=deepcopy(base);p['candidates'][0]['sell_available']='2000'
    add('liquidity',p,[-2,6,-4,-2],lots=True)
    p=deepcopy(base);p['candidates'][0]['holding_lower']='-1000'
    add('short_bound',p,[-1,6,-4,-2],lots=True)
    p=deepcopy(base);p['factors'][0]['lower']=-2.0;p['factors'][0]['upper']=3.0;p['factors'][0]['target']=5.0
    add('factor_boundary',p,[-5,6,-4,-2],lots=True)
    p=deepcopy(base);p['holdings'][0]['pending_quantity']='500'
    p['factors'][0]['scale']=2.0;p['factors'][0]['weight']=4.0
    # Keep the actual bounded baseline; different scaling cannot change an exact neutral hedge.
    p['candidates'][0]['trade_lower']='-15000';p['candidates'][0]['sell_available']='15000'
    add('scales',p,[-12,9,-6,-3],lots=True)
    two=[deepcopy(FACTORS[0]),deepcopy(FACTORS[2])]
    p=problem([4,2],[[1,0],[0,1]],['cash_bond','irs'],two)
    for c in p['candidates']:c['costs']['buy_per_reference']='1';c['costs']['sell_per_reference']='1'
    p['max_cost']=3.0;add('cost_budget',p,[-D('2.5'),-D('.5')])
    p=problem([3,5],[[1,1],[1,-1]],['cash_bond','irs'],two)
    add('coupled',p,[-4,1])
    p=problem([3,5],[[1,1],[1,-1]],['cash_bond','irs'],two)
    p['candidates'][0]['trade_lower']='-2000';add('coupled_bound',p,[-2,1])
    p=problem([D('2.5')],[[1]],['futures'],[FACTORS[0]])
    p['factors'][0]['lower']=-0.6;p['factors'][0]['upper']=0.6
    add('lot_tie',p,[-D('2.5')],lots=True)
    p=problem([D('.24')],[[1]],['cash_bond'],[FACTORS[0]]);p['candidates'][0]['lot']='100'
    add('notional_lot',p,[-D('.24')],lots=True)
    p=problem([3],[[1]],['cash_bond'],[FACTORS[0]]);p['candidates'][0]['trade_lower']=p['candidates'][0]['trade_upper']='-1000'
    add('fixed_trade',p,[-1])
    p=problem([3],[[1]],['cash_bond'],[FACTORS[0]]);p['holdings'][0]['risk']=deepcopy(p['candidates'][0]['risk']);p['expected_holdings']=['HEDGE.0']
    p['holdings'][0]['current_quantity']='2000';p['holdings'][0]['pending_quantity']='1000';p['candidates'][0]['holding_lower']='2000'
    add('held_candidate',p,[-1])
    p=problem([3],[[1]],['cash_bond'],[FACTORS[0]])
    fx={'from':'EUR','to':'USD','rate':'1.25','observation':{'id':'EURUSD','version':12},'snapshot_id':'SNAP.1'}
    for v in [p['holdings'][0]['risk'],p['candidates'][0]['risk']]:
        v['fx']=deepcopy(fx);v['reference_position']['currency']='EUR'
        for s in v['sensitivities']:s['position']['currency']='EUR'
    p['candidates'][0]['costs']['currency']='EUR';p['candidates'][0]['costs']['sell_per_reference']='2'
    add('fx',p,[-3])
    p=problem([0],[[1]],['cash_bond'],[FACTORS[0]]);p['settings']['cost_weight']=1.0
    add('zero_budget',p,[0])
    p=problem([3],[[1]],['cash_bond'],[FACTORS[0]]);p['candidates'][0]['trade_lower']='0';p['candidates'][0]['trade_upper']='0'
    add('zero_liquidity',p,[0])
    p=deepcopy(base);p['candidates']=[];add('aggregate_without_candidates',p,[],action='aggregate')
    # Explicit no-proposal diagnostics. Expectations precede candidate execution.
    p=problem([3],[[1]],['cash_bond'],[FACTORS[0]]);p['factors'][0]['lower']=p['factors'][0]['upper']=0.0;p['candidates'][0]['sell_available']='1000'
    add('infeasible',p,status='infeasible')
    p=problem([D('.5')],[[1]],['futures'],[FACTORS[0]]);p['factors'][0]['lower']=p['factors'][0]['upper']=0.0
    add('discrete_infeasible',p,[-D('.5')],status='discrete_infeasible',lots=True)
    p=problem([2,2],[[1,1],[2,2]],['cash_bond','irs'],two);add('singular',p,status='rank_deficient')
    p=problem([2,2],[[1,0],[0,0.000001]],['cash_bond','irs'],two);add('condition',p,status='ill_conditioned')
    p=deepcopy(base);p['settings']['max_work']=1;add('work',p,status='work_exhausted')
    p=deepcopy(base);p['candidates'][0]['holding_lower']='100000';p['candidates'][0]['holding_upper']='100001'
    add('empty_intersection',p,status='infeasible')
    def bad(name,mutate,error='invalid_input',action='solve'):
        p=deepcopy(base);mutate(p);add('invalid.'+name,p,error=error,action=action)
    bad('missing_holding',lambda p:p['holdings'].clear(),'missing_input')
    bad('missing_factor',lambda p:p['candidates'][0]['risk']['sensitivities'].pop(),'missing_input')
    bad('factor_order',lambda p:p['candidates'][0]['risk']['sensitivities'].reverse())
    bad('snapshot',lambda p:p['candidates'][0]['risk']['sensitivities'][0].update(snapshot_id='SNAP.2'))
    bad('entity',lambda p:p['candidates'][0]['risk'].update(entity='DESK.B'))
    bad('settlement',lambda p:p['candidates'][0]['risk'].update(settlement='2026-01-06'))
    bad('reference_size',lambda p:p['candidates'][0]['risk']['reference_position'].update(quantity='2000'))
    bad('normalization',lambda p:p['candidates'][0]['risk']['sensitivities'][0].update(value=7.0))
    bad('method',lambda p:p['candidates'][0]['risk']['sensitivities'][0].update(method='analytic'))
    bad('half_step',lambda p:p['candidates'][0]['risk']['sensitivities'][0].update(half_plus_pv=2.0))
    bad('model',lambda p:p['candidates'][0]['risk']['sensitivities'][0]['key']['factor'].update(model_version='OTHER'))
    bad('curve_version',lambda p:p['candidates'][0]['risk']['sensitivities'][0]['key']['coordinate']['curve'].update(version=99))
    bad('native_unit',lambda p:p['candidates'][0]['risk']['sensitivities'][0]['key'].update(native_unit='PercentRate'))
    bad('credit_entity',lambda p:p['candidates'][1]['risk']['credit_scope'][0].update(entity_id='OTHER'))
    bad('credit_recovery',lambda p:p['candidates'][1]['risk']['credit_scope'][0].update(recovery_fraction=0.5))
    bad('credit_document',lambda p:p['candidates'][1]['risk']['credit_scope'][0]['document'].update(version=8))
    bad('basis_to_credit',lambda p:p['candidates'][3]['risk']['sensitivities'][3].update(key=deepcopy(p['factors'][1]['key'])))
    bad('expected_duplicate',lambda p:p['expected_holdings'].append('ORIGIN'))
    bad('candidate_duplicate',lambda p:p['candidates'].__setitem__(1,deepcopy(p['candidates'][0])))
    bad('factor_duplicate',lambda p:p['factors'].__setitem__(1,deepcopy(p['factors'][0])))
    bad('eligibility',lambda p:p['candidates'][0]['eligibility'].update(eligible=False))
    bad('negative_cost',lambda p:p['candidates'][0]['costs'].update(sell_per_reference='-1'))
    bad('cost_currency',lambda p:p['candidates'][0]['costs'].update(currency='GBP'))
    bad('negative_liquidity',lambda p:p['candidates'][0].update(buy_available='-1'))
    bad('interval',lambda p:p['candidates'][0].update(trade_lower='100000'))
    bad('lot_zero',lambda p:p['candidates'][0].update(lot='0'))
    bad('future_lot_missing',lambda p:p['candidates'][3].update(lot=None))
    bad('future_lot_fraction',lambda p:p['candidates'][3].update(lot='0.5'))
    bad('pending_fraction_contract',lambda p:(p['holdings'].__setitem__(0,{'risk':deepcopy(p['candidates'][3]['risk']),'current_quantity':'1','pending_quantity':'0.5'}),p.update(expected_holdings=['HEDGE.3'])))
    bad('no_candidates',lambda p:p['candidates'].clear())
    bad('five_candidates',lambda p:p['candidates'].append(deepcopy(p['candidates'][0])),'resource_limit')
    bad('seventeen_factors',lambda p:p['factors'].extend(deepcopy(p['factors'])*4),'resource_limit')
    bad('solver_tolerance',lambda p:p['settings'].update(solver_tolerance=0.01))
    bad('work_zero',lambda p:p['settings'].update(max_work=0))
    bad('missing_fx',lambda p:(p['candidates'][0]['risk']['reference_position'].update(currency='EUR'),[s['position'].update(currency='EUR') for s in p['candidates'][0]['risk']['sensitivities']]),'missing_input')
    bad('identity_fx',lambda p:p['candidates'][0]['risk'].update(fx={'from':'USD','to':'USD','rate':'1','observation':{'id':'BAD','version':1},'snapshot_id':'SNAP.1'}))
    # Structural controls with exact field/unit loss, tested before native admission.
    for name,owner,key,value in [('numeric_quantity',['candidates',0],'buy_available',1000),('unknown_solver', ['settings'],'algorithm','guess'),
                               ('unknown_risk',['candidates',0,'risk'],'dv01',99),('zero_revision',['position_state'],'version',0)]:
        def mutate(p,owner=owner,key=key,value=value):
            for part in owner:p=p[part]
            p[key]=value
        bad(name,mutate)
    return {'certified':False,'provenance':'Fictional full five-PV supplied-risk controls; independent Decimal60 analytic and exact native-lattice arithmetic; no instrument pricing, market, order or latency certificate.','cases':cases}, {'certified':False,'precision':60,'provenance':'Independent analytic continuous solutions and original finite-lattice arithmetic; no candidate output consumed.','cases':refs}

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');a=parser.parse_args()
    controls,reference=generate()
    for name,value in [('constrained-hedge.json',controls),('constrained-hedge-reference.json',reference)]:
        path=FOLDER/name;content=(json.dumps(value,indent=2)+'\n').encode()
        if a.write:
            if path.exists():raise SystemExit('Refusing to overwrite original control '+str(path))
            path.write_bytes(content)
        elif path.read_bytes()!=content:raise ValueError('Frozen original control differs: '+name)
    print(json.dumps({'requests':len(controls['cases']),'independent_controls':len(reference['cases']),'certified':False}))
