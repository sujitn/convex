"""Separately declared full-input scenario admission controls; no native outputs."""
import argparse,copy,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT/'tests/fixtures/library/mixed-scenario-admitted.json'
TARGET=ROOT/'tests/fixtures/library/mixed-scenario-boundaries.json'
def generate():
 original=json.loads(SOURCE.read_bytes())['cases'][0]['request'];cases=[]
 def add(name,change,error='invalid_input'):
  request=copy.deepcopy(original);change(request['operation']['action']['input']);cases.append(dict(id='mixed.boundary.'+name,request=request,expected_error=error))
 add('reordered_holdings',lambda d:d['holdings'].reverse())
 add('unknown_holding',lambda d:d['holdings'][0].update(id='unknown-position'))
 add('nonpositive_reference',lambda d:d['book']['positions'][0]['current'].update(reference_quantity='0'))
 add('wrong_currency',lambda d:d['book']['positions'][0]['current'].update(currency='EUR'))
 add('too_many_moves',lambda d:d['book']['moves'].extend(copy.deepcopy(d['book']['moves'][:5])))
 add('graph_ceiling',lambda d:d['book']['policy'].update(max_graph_evaluations=34))
 add('decimal_overflow',lambda d:d['book']['positions'][0].update(pending_quantity='79228162514264337593543950336'))
 def alias(d):
  m=copy.deepcopy(d['book']['moves'][-1]);m['key']['coordinate']['version']+=1;d['book']['moves'].append(m)
 add('raw_observation_alias',alias)
 def zero_alias(d):
  m=copy.deepcopy(d['book']['moves'][0]);m['key']['coordinate']['node']=1;d['book']['moves'].append(m)
 add('parallel_and_node',zero_alias)
 def quote_alias(d):
  m=copy.deepcopy(d['book']['moves'][0]);k=m['key'];k['coordinate'].update(kind='quote',helper_id='complete-helper',quote_version=1);k['coordinate'].pop('node');k['response']='rebuild_dependents';k['factor'].update(quote_type='deposit_rate',basis='market_quote_recalibrated');d['book']['moves'].append(m)
 add('quote_and_output_zero',quote_alias)
 def unrelated(d):
  m=d['book']['moves'][7];m['key']['coordinate']['input_id']='UNCONSUMED.SECURITY';m['key']['factor']['target']['instrument_id']='UNCONSUMED.SECURITY'
 add('unconsumed_raw',unrelated)
 add('oas_currency',lambda d:d['book']['moves'][9]['key']['factor'].update(currency='EUR'))
 add('model_currency',lambda d:d['book']['moves'][10]['key']['factor'].update(currency='EUR'))
 add('oas_native_percent',lambda d:d['book']['moves'][9]['key'].update(native_unit='PercentRate'))
 def raw_percent(d):
  m=copy.deepcopy(d['book']['moves'][9]);k=m['key'];id_=d['book']['positions'][0]['identity']['instrument'];k['coordinate'].update(input_id=id_['id'],version=id_['version']);k['factor'].update(quote_type='z_spread',target=dict(kind='instrument',instrument_id=id_['id']));k['native_unit']='PercentRate';d['book']['moves'].append(m)
 add('bond_raw_native_percent',raw_percent)
 def extra(d):
  d['book'].update(reporting_currency='EUR');pin={'from':'USD','to':'EUR','rate':'0.9','observation':{'id':'FX.SCENARIO','version':1},'snapshot_id':d['book']['snapshot_id']};d['book']['fx']=[pin,copy.deepcopy(pin)]
 add('duplicate_fx',extra)
 def negative_fx(d):
  extra(d);d['book']['fx']=d['book']['fx'][:1];d['book']['fx'][0]['rate']='-1'
 add('negative_fx',negative_fx)
 def optional_identity(d):d['book']['positions'][-2]['identity']['instrument']['version']+=1
 add('optional_identity',optional_identity)
 def cds_identity(d):d['book']['positions'][-3]['identity']['instrument']['version']+=1
 add('cds_identity',cds_identity)
 def swap_reference(d):d['book']['positions'][1]['current']['reference_quantity']='100'
 add('swap_reference',swap_reference)
 return dict(certified=False,source_reads={'tests/fixtures/library/mixed-scenario-admitted.json':hashlib.sha256(SOURCE.read_bytes()).hexdigest()},qualification='Complete original input failures; no native financial output, original positive values and tolerances unchanged.',cases=cases)
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,separators=(',',':'))+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing overwrite')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Boundary declarations changed')
 print(json.dumps(dict(status='passed',cases=len(v['cases']),certified=False)))
