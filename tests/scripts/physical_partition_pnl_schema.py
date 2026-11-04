"""Strict complete original custody and invoice legal inputs, without cached marks."""
import argparse,copy,hashlib,json,subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats
from physical_partition_pnl_final_census import source_cases
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
    root=['operation','input','physical_partitions',0]
    reject(root,['split','future','delivery_id','receipt_ids'])
    reject(root+['split'],['id','parent','children','effective_date','known_at','source','applied_points'])
    reject(root+['split','source'],['id','version'])
    reject(['operation','input','claim_transfers',0],['id','origin','key','proposal_id','entitlement_source','from','to','fraction','cutoff','source','applied_points'])
    for r in mutations:
        if v.is_valid(r):raise ValueError('Incomplete or ignored legal input accepted')
    return dict(status='passed',financial_books=len(positive),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
