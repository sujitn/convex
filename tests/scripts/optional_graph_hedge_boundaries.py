"""Direct original model owners and graph ProductRisk, reusing immutable independent answers."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
SOURCES=['tests/fixtures/library/optional-graph-hedge-api.json','tests/fixtures/library/optional-graph-hedge-reference.json']
TARGET='tests/fixtures/library/optional-graph-hedge-direct.json'
def generate():
    original=json.loads((ROOT/SOURCES[0]).read_bytes())['cases'][0];req=original['request'];op=req['operation']
    expected=json.loads((ROOT/SOURCES[1]).read_bytes())['cases'][0]['expected'];cases=[]
    for i in [2,3]:
        f=op['input']['factors'][i];control=expected['factors'][i];parameter='mean_reversion' if control['kind']=='a' else 'volatility';h=f['settings']['bump']
        for j,move in enumerate([0,h,-h,h/2,-h/2]):
            r=deepcopy(req);r['operation']=dict(kind='optional_bond',input=deepcopy(op['product']['input']),action=dict(kind='model_scenario',oas_decimal=op['product']['oas_decimal'],parameter=parameter,delta=move))
            cases.append(dict(id='optional_graph.direct.'+parameter+'.scenario.'+str(j),request=r,expected_error=None,financial=control['valuations'][j]))
        for label,move,indexes in [('h',h,[0,1,2]),('half',h/2,[0,3,4])]:
            r=deepcopy(req);r['operation']=dict(kind='optional_bond',input=deepcopy(op['product']['input']),action=dict(kind='model_risk',oas_decimal=op['product']['oas_decimal'],parameter=parameter,bump=move,settings=deepcopy(op['product']['policies']['model'])))
            cases.append(dict(id='optional_graph.direct.'+parameter+'.risk.'+label,request=r,expected_error=None,financial=[control['valuations'][j] for j in indexes]))
    for i in [0,3]:
        r=deepcopy(req);r['operation']=dict(kind='calibration',input=deepcopy(op['graph']),action=dict(kind='product_risk',product=deepcopy(op['product']),key=deepcopy(op['input']['factors'][i]['key']),position=deepcopy(op['input']['reference_position']),snapshot_id=op['input']['snapshot_id'],settings=deepcopy(op['input']['factors'][i]['settings'])))
        cases.append(dict(id='optional_graph.direct.product_risk.'+str(i),request=r,expected_error=None,financial=deepcopy(expected['factors'][i])))
    return dict(certified=False,source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},cases=cases)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode();out=ROOT/TARGET
    if a.write:
        if out.exists():raise ValueError('Refusing to overwrite original direct controls')
        out.write_bytes(raw)
    elif out.read_bytes()!=raw:raise ValueError('Original direct controls differ')
    print(json.dumps(dict(status='passed',cases=16,certified=False)))
