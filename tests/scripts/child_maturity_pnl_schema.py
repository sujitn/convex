"""Strict complete legal ancestry, actual-fill disposal and receipt allocation schema."""
import argparse,copy,hashlib,json,subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats
from child_maturity_pnl_admitted_census import source_cases
def validate(binary):
    raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw)
    Draft202012Validator.check_schema(schema);v=Draft202012Validator(schema,format_checker=formats())
    positive=[c for c in source_cases() if c['expected_error'] is None]
    for c in positive:v.validate(c['request'])
    mutations=[]
    def reject(path,fields):
        for field in fields+['unexpected']:
            r=copy.deepcopy(positive[0]['request']);n=r
            for k in path:n=n[k]
            if field=='unexpected':n['cached_received_face']='0'
            else:del n[field]
            mutations.append(r)
    root=['operation','input','dated_entitlements',0]
    reject(root,['origin','key','proposal_id','entitlement_source','residual','source','applied_points'])
    reject(root+['residual'],['id','fraction','cutoff'])
    reject(root+['residual','cutoff'],['date','known_at','receipt_ids'])
    reject(root+['source'],['id','version'])
    for r in mutations:
        if v.is_valid(r):raise ValueError('Incomplete or cached custody input accepted')
    return dict(status='passed',financial_books=len(positive),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
