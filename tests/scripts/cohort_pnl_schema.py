"""Strict complete cohort/cash/physical schema, with no cached-value shortcuts."""
import argparse,copy,hashlib,json,subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats
from cohort_pnl_census import source_cases
def validate(binary):
    raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator=Draft202012Validator(schema,format_checker=formats())
    positives=[c for c in source_cases() if c['expected_error'] is None]
    for c in positives:validator.validate(c['request'])
    base=positives[0]['request'];mutations=[]
    def reject(path,keys,source=base):
        def node(r):
            for k in path:r=r[k]
            return r
        for k in keys:r=copy.deepcopy(source);del node(r)[k];mutations.append(r)
        r=copy.deepcopy(source);node(r)['cached_pnl']=0;mutations.append(r)
    inp=['operation','input'];book=inp+['book'];before=book+['before'];point=before+['point']
    reject(inp,['book','cohorts','physical_deliveries'])
    reject(inp+['cohorts',0],['trade_id','quantity','acquisition','entitlements'])
    reject(inp+['cohorts',0,'acquisition'],['kind'])
    reject(inp+['cohorts',0,'entitlements',0],['key','entitled','source'])
    reject(inp+['cohorts',0,'entitlements',0,'key'],['kind','effective_date','ordinal'])
    reject(inp+['cohorts',0,'entitlements',0,'source'],['id','version'])
    reject(book,['resources','markets','holdings','before','steps','standalone','reporting_currency','max_points','max_product_evaluations'])
    reject(before,['point','market','holdings','additional_fixings','additional_publications'])
    reject(point,['snapshot_id','manifest','valuation_date','known_at','positions','fills','fx'])
    reject(point+['positions',0],['identity','current','pending_quantity','cash_discount','overdue'])
    reject(book+['holdings',0],['id','product','lifecycle'])
    reject(book+['holdings',0,'lifecycle'],['trade_id','version','instrument','position','known_at','history','events'])
    reject(book+['steps',0],['cause','after'])
    reject(book+['standalone',0],['kind','information'])
    reject(book+['markets',0],['curves'])
    reject(point+['positions',0,'current'],['quantity','reference_quantity','quantity_unit','currency'])
    physical=next(c['request'] for c in positives if c['request']['operation']['input']['physical_deliveries'])
    reject(inp+['physical_deliveries',0],['future','asset','instrument'],physical)
    reject(inp+['cohorts',1,'acquisition'],['kind','future'],physical)
    acquired=next(c['request'] for c in positives if any(o['acquisition']['kind']=='fill' for o in c['request']['operation']['input']['cohorts']))
    i=next(i for i,o in enumerate(acquired['operation']['input']['cohorts']) if o['acquisition']['kind']=='fill')
    reject(inp+['cohorts',i,'acquisition'],['kind','fill_id'],acquired)
    for r in mutations:
        if validator.is_valid(r):raise ValueError('Schema accepted missing/ignored original economics or ownership')
    return dict(status='passed',financial_books=len(positives),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
