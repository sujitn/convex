"""Strict full original proposal, legal transfer and receipt cutoffs."""
import argparse,copy,hashlib,json,subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats
from claim_pnl_bound_census import source_cases
def validate(binary):
    raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw)
    Draft202012Validator.check_schema(schema);validator=Draft202012Validator(schema,format_checker=formats())
    positive=[c for c in source_cases() if c['expected_error'] is None]
    for c in positive:validator.validate(c['request'])
    mutations=[];request=positive[0]['request']
    def reject(path,fields):
        for field in fields:
            r=copy.deepcopy(request);n=r
            for k in path:n=n[k]
            del n[field];mutations.append(r)
        r=copy.deepcopy(request);n=r
        for k in path:n=n[k]
        n['cached_price']=0;mutations.append(r)
    root=['operation','input','claim_transfers',0]
    reject(root,['id','origin','key','proposal_id','entitlement_source','from','to','fraction','cutoff','source','applied_points'])
    reject(root+['cutoff'],['date','known_at','receipt_ids'])
    reject(root+['key'],['kind','effective_date','ordinal'])
    reject(root+['source'],['id','version'])
    for r in mutations:
        if validator.is_valid(r):raise ValueError('Missing or ignored original legal fact accepted')
    return dict(status='passed',financial_books=len(positive),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
