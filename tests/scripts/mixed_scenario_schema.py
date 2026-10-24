"""Strict actual mixed-book full-input request schema, with omitted/ignored data controls."""
import argparse,copy,hashlib,json,subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats
from mixed_scenario_census import source_cases

def validate(binary):
 raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw);Draft202012Validator.check_schema(schema)
 validator=Draft202012Validator(schema,format_checker=formats());positives=[c for c in source_cases() if c['expected_error'] is None]
 for c in positives:validator.validate(c['request'])
 original=positives[0]['request'];mutations=[]
 def reject(path,keys):
  def node(r):
   for key in path:r=r[key]
   return r
  for key in keys:
   r=copy.deepcopy(original);del node(r)[key];mutations.append(r)
  r=copy.deepcopy(original);node(r)['cached_pnl']=0;mutations.append(r)
 data=['operation','action','input'];book=data+['book']
 reject(data,['book','holdings']);reject(book,['snapshot_id','positions','moves','reporting_currency','fx','policy'])
 reject(book+['policy'],['max_graph_evaluations','max_product_evaluations'])
 reject(book+['positions',0],['identity','current','pending_quantity']);reject(book+['positions',0,'identity'],['id','instrument','economics','calculation','entity'])
 reject(book+['positions',0,'current'],['quantity','reference_quantity','quantity_unit','currency'])
 reject(book+['moves',0],['key','delta']);reject(book+['moves',0,'key'],['factor','coordinate','native_unit','response','unit'])
 reject(data+['holdings',0],['id','product']);reject(data+['holdings',0,'product'],['product'])
 reject(data+['holdings',-1,'product','product'],['input','discount','policy'])
 reject(data+['holdings',-1,'product','product','input','members',0],['instrument','stamp','bond','clean_price_per100','quote_stamp','financing','coupon_carry'])
 for r in mutations:
  if validator.is_valid(r):raise ValueError('Schema admitted missing/ignored full inputs')
 return dict(status='passed',positive_books=len(positives),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
