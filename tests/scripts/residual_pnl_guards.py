"""Add complete independently declared legal/proposal boundary failures.

Keep all full financial books, every original numerical value/bound and prior
failed input unchanged. These extra whole requests reject fully exhausted prior
cash, foreign receipt ownership, duplicate residual identities, missing legal
source, and proposal information unavailable at actual acquisition.
"""
import argparse,copy,hashlib,json
from pathlib import Path
from residual_pnl_reference import original
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
SOURCE=FOLDER/'residual-pnl-original-units.json';TARGET=SOURCE.with_name('residual-pnl-admitted.json')
def generate():
    data=json.loads(SOURCE.read_bytes());base=next(c for c in data['cases'] if c['id']=='residual.recovery.short.1.held')
    def reject(name,mutation,source=base):
        case=copy.deepcopy(source);case.update(id='residual.invalid.'+name,expected_error='invalid_input');case.pop('expected',None)
        mutation(case['request']['operation']['input']);data['cases'].append(case)
    def entitlement(s):return next(e for o in s['cohorts'] for e in o['entitlements'] if e.get('residual'))
    full=original('cohort.invalid.acquired_historically_full_paid_claim')
    def exhausted(s):
        owner=s['cohorts'][0];ent=next(e for e in owner['entitlements'] if e['key']['kind']=='recovery')
        original_ent=entitlement(base['request']['operation']['input']);ent['residual']=copy.deepcopy(original_ent['residual'])
    reject('fully_paid_before_transfer',exhausted,full)
    def duplicate(s):
        owner=s['cohorts'][0];ent=entitlement(s);other=next(e for e in owner['entitlements'] if e['key']['kind']=='default_accrual')
        other['residual']=copy.deepcopy(ent['residual']);other['residual']['cutoff']['receipt_ids']=[]
    reject('duplicate_residual_identity',duplicate)
    def foreign(s):
        owner=s['cohorts'][0];ent=entitlement(s);other=next(e for e in owner['entitlements'] if e['key']['kind']=='default_accrual')
        other['residual']=ent.pop('residual');ent['entitled']=False
    reject('foreign_original_receipt_claim',foreign)
    reject('missing_legal_source',lambda s:entitlement(s).pop('source'))
    def unknown(s):
        for h in s['book']['holdings']:
            for step in h['lifecycle']['events']:
                if step['event']['action']['kind']=='recovery':step['event']['known_at']='2026-10-15T11:00:00Z'
        for binding in [s['book']['before']]+[x['after'] for x in s['book']['steps']]:
            for fill in binding['point']['fills']:
                if fill['id']=='ACTUAL.DEFAULT.CLAIM.ACQUISITION':fill['known_at']='2026-10-15T10:00:00Z'
        entitlement(s)['residual']['cutoff']['known_at']='2026-10-15T10:00:00Z'
    reject('proposal_unknown_at_acquisition',unknown)
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    raw=(json.dumps(generate(),indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original legal/proposal controls changed')
    print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
