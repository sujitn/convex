"""Complete available independent custody experiments and original receipt provenance.

Retain the declared full financial bodies and bounds. Consumed custody parents
have no owned receipt allocations, while every full original receipt remains
visible. Add complete baseline-delivered long/short books with three actual
Position changes and three independently available Position information sets.
All original product/model values are copied from the independent full reference;
security conservation and actual fill cash are derived separately in Decimal60.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
import custody_lineage_pnl_reference as original
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_bindings as binding
import cohort_pnl_cash_dates as dates
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
import lot_pnl_reference as lots

SOURCE=original.TARGET;TARGET=SOURCE.with_name('custody-lineage-pnl-bound.json')
clone=copy.deepcopy;ZERO=D(0)

def independent(case):
    case=clone(case);spec=case['request']['operation']['input'];expanded=states.expand(case['request'])
    before=clone(r.sets(expanded)[len(expanded['steps'])]);before['point']['fills']=[]
    name=case['id']+'.available';owners=clone(spec['cohorts']);parts=clone(spec['physical_partitions']);disposals=clone(spec['physical_disposals'])
    for j,p in enumerate(before['point']['positions']):
        if j:p['current']['quantity']=owners[1]['quantity'] if j==1 else '0'
    book={k:clone(v) for k,v in expanded.items() if k not in ['before','steps','standalone']}
    book.update(before=r.c.repoint(before,name+'.before'),steps=[],standalone=[])
    for i,label in enumerate(['root','successors','disposal']):
        info=r.c.repoint(before,name+'.'+label)
        book['steps'].append(dict(cause='position',after=info))
        book['standalone'].append(dict(kind='available',information=r.c.repoint(info,name+'.independent.'+label)))
    parts[0]['split'].update(effective_date=before['point']['valuation_date'],known_at=before['point']['known_at'],applied_points=[1,2,3,4,5,6])
    parts[1]['split'].update(effective_date=before['point']['valuation_date'],known_at=before['point']['known_at'],applied_points=[2,3,5,6])
    receipts=original.receipt_events(before)
    for p in parts:p['receipt_ids']=[e['action']['terms']['receipt_id'] for e in receipts]
    target=4;owned=D(owners[target]['quantity']);fraction=D('0.5')
    allocations=[dict(receipt_id=e['action']['terms']['receipt_id'],face=str(D(e['action']['terms']['face'])*owned/D(owners[1]['quantity'])*fraction)) for e in receipts]
    info=book['steps'][2]['after'];face=sum((D(a['face']) for a in allocations),ZERO)
    r.execution(info,target,disposals[0]['fill_id'],-face,D(1 if face>0 else -1)*D('16811.25'))
    book['standalone'][2]['information']['point']['fills']=clone(info['point']['fills'])
    disposals[0].update(effective_date=before['point']['valuation_date'],known_at=before['point']['known_at'],receipt_ids=[e['action']['terms']['receipt_id'] for e in receipts],receipts=allocations,applied_points=[3,6])
    expected=clone(case['expected']);root=clone(expected['trades'][1]['marks'][len(expanded['steps'])]['physical']);root.pop('partition',None);root.pop('lineage',None);root.pop('disposals',None)
    root.update(invoice_excluded=False,outstanding_invoice=D(root['invoice']['amount'])-D(root['invoice']['recorded']),invoice_present_value=D(root['invoice']['amount'])-D(root['invoice']['recorded']))
    for j,t in enumerate(expected['trades']):
        m=clone(t['marks'][len(expanded['steps'])]);m.update(transferred=[],consideration=ZERO,fees=ZERO,executed=ZERO,executed_marking=ZERO)
        t['marks']=[clone(m) for _ in range(7)]
    for i,info in enumerate(r.sets(book)):
        balances,removed,acquired=original.custody(info,owners,parts,disposals,i)
        for j in range(1,len(owners)):
            m=expected['trades'][j]['marks'][i];b=balances[owners[j]['trade_id']];held=sum(b['receipts'].values(),ZERO)
            info['point']['positions'][j]['current']['quantity']=str(held)
            p=clone(root) if owners[j]['trade_id'] in acquired else None
            if p:
                p.update(proposed_face=held+b['pending'],received_face=held,outstanding_face=b['pending'],outstanding_asset_present_value=D(m['reference'])*b['pending']/D(info['point']['positions'][j]['current']['reference_quantity']))
                if b['facts']:
                    p['partition']=dict(facts=clone(b['facts']),original_proposed_face=D(owners[1]['quantity']),original_received_face=D(owners[1]['quantity']),original_outstanding_face=ZERO,fraction=held/D(owners[1]['quantity']),receipts=[] if any(x['split']['parent']==owners[j]['trade_id'] for x in b['lineage']) else [dict(event=clone(e),face=b['receipts'].get(e['action']['terms']['receipt_id'],ZERO)) for e in receipts])
                if len(b['lineage'])>1:p['lineage']=clone(b['lineage'])
                if removed[owners[j]['trade_id']]:p['disposals']=clone(removed[owners[j]['trade_id']])
                if j!=1:p.update(invoice_excluded=True,outstanding_invoice=ZERO,invoice_present_value=ZERO)
            m['physical']=p;m['recorded']=D(root['invoice']['recorded']) if j==1 else ZERO;m['proposed']=D(root['invoice']['amount']) if j==1 else ZERO
            fills=[f for f in info['point']['fills'] if f['trade_id']==owners[j]['trade_id']]
            m['executed']=sum((D(f['quantity']) for f in fills),ZERO);m['executed_marking']=D(m['reference'])*m['executed']/D(info['point']['positions'][j]['current']['reference_quantity'])
            m['consideration']=sum((D(f['consideration']['value']) for f in fills),ZERO);m['fees']=sum((D(f['fees']['value']) for f in fills),ZERO)
            value=D(m['reference'])*(held+b['pending'])/D(info['point']['positions'][j]['current']['reference_quantity'])+(D(root['invoice_present_value']) if j==1 else ZERO)
            m['local']=dict(current=value,pending=ZERO,projected=value);m['reporting']={k:v*D(m['fx']) for k,v in m['local'].items()}
    binding.explain(expected,dict(cohorts=owners),book)
    request=residual.reencode(book,owners,spec['physical_deliveries']);request['operation']['input'].update(lot_splits=[],claim_transfers=[],physical_partitions=parts,physical_disposals=disposals)
    answer=dict(id=name,request=request,expected_error=None,expected=expected)
    expected['delivery_comparisons']=dates.comparisons(answer);canonical.normalize(answer);return answer

def generate():
    data=json.loads(SOURCE.read_bytes())
    for c in data['cases']:
        if c['expected_error'] is not None:continue
        spec=c['request']['operation']['input']
        for j,t in enumerate(c['expected']['trades']):
            for i,m in enumerate(t['marks']):
                if m['physical'] and m['physical'].get('partition') and any(p['split']['parent']==spec['cohorts'][j]['trade_id'] and i in p['split']['applied_points'] for p in spec['physical_partitions']):m['physical']['partition']['receipts']=[]
    with localcontext() as ctx:
        ctx.prec=60
        extra=[independent(c) for c in data['cases'] if c['expected_error'] is None and c['id'].endswith('.independent_full')]
    data['cases']+=extra
    def reject(label,fn):
        c=clone(extra[0]);c.update(id='custody.invalid.available.'+label,expected_error='invalid_input');c.pop('expected');fn(c['request']['operation']['input']);data['cases'].append(c)
    reject('missing_ancestor',lambda s:s['physical_partitions'][0]['split']['applied_points'].remove(5))
    reject('missing_actual_fill',lambda s:s['book']['standalone'][2]['information']['point']['fills'].clear())
    reject('foreign_actual_receipt',lambda s:s['physical_disposals'][0]['receipt_ids'].__setitem__(0,'ABSENT'))
    reject('original_revision',lambda s:s['book']['before']['point']['positions'][4]['identity']['instrument'].update(version=99))
    reject('ignored_balance',lambda s:s['physical_disposals'][0].update(cached_received_face='100'))
    reject('receipt_work_bound',lambda s:s['physical_disposals'][0].update(receipts=s['physical_disposals'][0]['receipts']*1366))
    for c in data['cases']:
        c['input_nodes']=lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete declared custody and independent books differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
