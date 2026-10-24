"""Strict complete legal amendments/splits, without scalar or implicit facts."""
import argparse,copy,hashlib,json,subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats
from lot_pnl_census import source_cases
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
        r=copy.deepcopy(source);node(r)['cached_price']=0;mutations.append(r)
    request=positive[0]['request'];inp=['operation','input']
    reject(request,inp+['lot_splits',0],['id','parent','children','effective_date','known_at','source','applied_points'])
    spec=request['operation']['input'];j=next(i for i,o in enumerate(spec['cohorts']) if o['acquisition']['kind']=='lot_split')
    reject(request,inp+['cohorts',j,'acquisition'],['kind','split_id'])
    b=spec['book'];i,j=next((i,j) for i,h in enumerate(b['holdings']) for j,s in enumerate(h['lifecycle']['events']) if s['event']['action']['kind']=='bond_amendment')
    reject(request,inp+['book','holdings',i,'lifecycle','events',j,'event','action','terms'],['prior_version','version','agreement','terms'])
    for r in mutations:
        if validator.is_valid(r):raise ValueError('Accepted missing or ignored original legal fact')
    return dict(status='passed',financial_books=len(positive),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
