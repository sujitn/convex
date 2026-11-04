"""Strict complete causal native request schema; no economic shortcuts admitted."""
import argparse,copy,hashlib,json,subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats
from causal_pnl_census import source_cases

def validate(binary):
 raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw);Draft202012Validator.check_schema(schema);validator=Draft202012Validator(schema,format_checker=formats());positives=[c for c in source_cases() if c['expected_error'] is None]
 for c in positives:validator.validate(c['request'])
 base=positives[0]['request'];mutations=[]
 def reject(path,keys):
  def node(r):
   for k in path:r=r[k]
   return r
  for k in keys:r=copy.deepcopy(base);del node(r)[k];mutations.append(r)
  r=copy.deepcopy(base);node(r)['cached_pnl']=0;mutations.append(r)
 inp=['operation','input'];before=inp+['before'];point=before+['point']
 reject(inp,['resources','markets','holdings','before','steps','standalone','reporting_currency','max_points','max_product_evaluations']);reject(before,['point','market','holdings','additional_fixings','additional_publications']);reject(point,['snapshot_id','manifest','valuation_date','known_at','positions','fills','fx']);reject(point+['positions',0],['identity','current','pending_quantity','cash_discount','overdue']);reject(inp+['holdings',0],['id','product','lifecycle']);reject(inp+['holdings',0,'lifecycle'],['trade_id','version','instrument','position','known_at','history','events']);reject(inp+['steps',0],['cause','after']);reject(inp+['standalone',0],['kind','information']);reject(inp+['markets',0],['curves']);reject(point+['positions',0,'current'],['quantity','reference_quantity','quantity_unit','currency'])
 for r in mutations:
  if validator.is_valid(r):raise ValueError('Schema accepted missing/ignored full economic information')
 return dict(status='passed',financial_books=len(positives),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
