#!/usr/bin/env python3
"""Produce complete native control requests; expectations are independently declared."""
import argparse
import copy
import json
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools'))
import core_roll_workflow as example
import core_snapshot_workflow as core
import streaming_analytics as analytics


def fixture():
    capture,plan,factors=core.prepare()
    plan['definition']['version']=3
    config=dict(risks={},pnl='pnl',plan=copy.deepcopy(plan),bindings=[],hedge=dict(operation=dict(problem=dict(factors=[]))))
    original=capture['references'][1]['command']['master_changes'][0]['definition']['definition']
    for name,calc in original['calculations'].items():
        if name!='pnl':config['risks'][calc['request']['operation']['input']['identity']['id']]=name
    for key,pins in plan['curve_sets'].items():
        for pin in pins:
            r=next(r for r in capture['curves'] if r['curve_id']==pin['id'] and r['version']==pin['version'])
            config['bindings'].append(dict(set=key,curve_id=r['curve_id'],source=r['source'],recipe_key=r['recipe']['key'],follow=key=='market.1',max_age_ms=5000))
    value=example.definition(capture,factors,config)
    curves,fixings=copy.deepcopy(capture['curves']),copy.deepcopy(capture['fixings'])
    controls={}
    core_control=None
    def request():
        calc=value['calculations']['risk.0']
        req=copy.deepcopy(calc['request'])
        req['inputs']=copy.deepcopy(value['reference_inputs'])
        req['inputs']['fixings']=[example.native_observation(r) for r in fixings]
        pins=config['plan']['curve_sets'][calc['curve_sets'][0]]
        req['inputs']['curves']=[next(r['recipe'] for r in curves if r['curve_id']==p['id'] and r['version']==p['version']) for p in pins]
        return req
    controls['opening']=request()
    for day,observed,key in [('2026-10-08','2026-10-07','coupon'),('2026-10-09','2026-10-08','maturity'),('2026-10-13','2026-10-09','due')]:
        observation=example.fixing(observed,day)
        value,config,fresh=example.roll(value,config,config['plan'],curves,day,observation,cancel=key=='maturity')
        curves.extend(fresh);fixings.append(observation)
        controls[key]=request()
        if key=='maturity':
            core_control=dict(definition=copy.deepcopy(value),plan=copy.deepcopy(config['plan']),curves=copy.deepcopy(curves),fixings=copy.deepcopy(fixings))
    return dict(schema_version=1,provenance='Complete supplied native requests; see separate Decimal80 date-rolls-reference.json for expectations',requests=controls,core=core_control)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('mode',choices=('write','check'));args=parser.parse_args()
    path=ROOT/'tests/fixtures/library/date-rolls-requests.json'
    value=fixture()
    core_value=value.pop('core')
    raw=(json.dumps(value,indent=2,sort_keys=True)+'\n').encode()
    core_raw=(json.dumps(core_value,indent=2,sort_keys=True)+'\n').encode()
    core_path=ROOT/'tests/fixtures/library/date-rolls-core.json'
    if args.mode=='write':
        if path.exists():raise RuntimeError('Control requests already exist; never refresh silently')
        path.write_bytes(raw)
        core_path.write_bytes(core_raw)
    elif path.read_bytes()!=raw or core_path.read_bytes()!=core_raw:raise RuntimeError('Control request generation changed')
    print(json.dumps(dict(status='passed',mode=args.mode)))
