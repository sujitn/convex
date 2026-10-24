"""Strict residual legal ownership and original actual partial receipt schema."""
import argparse,copy,hashlib,json,subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats
from residual_pnl_census import source_cases
def validate(binary):
    raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw)
    Draft202012Validator.check_schema(schema);validator=Draft202012Validator(schema,format_checker=formats())
    positive=[c for c in source_cases() if c['expected_error'] is None]
    for c in positive:validator.validate(c['request'])
    mutations=[]
    def reject(source,path,fields):
        def node(r):
            for k in path:r=r[k]
            return r
        for field in fields:
            r=copy.deepcopy(source);del node(r)[field];mutations.append(r)
        r=copy.deepcopy(source);node(r)['cached_cash']=0;mutations.append(r)
    request=positive[0]['request'];inp=['operation','input'];spec=request['operation']['input']
    i,j=next((i,j) for i,o in enumerate(spec['cohorts']) for j,e in enumerate(o['entitlements']) if 'residual' in e)
    path=inp+['cohorts',i,'entitlements',j,'residual']
    reject(request,path,['id','fraction','cutoff']);reject(request,path+['cutoff'],['date','known_at','receipt_ids'])
    physical=next(c['request'] for c in positive if c['request']['operation']['input']['physical_deliveries'])
    b=physical['operation']['input']['book'];i,j=next((i,j) for i,h in enumerate(b['holdings']) for j,e in enumerate(h['lifecycle']['events']) if e['event']['action']['kind']=='security_receipt')
    reject(physical,inp+['book','holdings',i,'lifecycle','events',j,'event','action','terms'],['obligation','instrument','face','receipt_id','source'])
    for r in mutations:
        if validator.is_valid(r):raise ValueError('Accepted missing/ignored ownership or actual receipt facts')
    return dict(status='passed',financial_books=len(positive),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
