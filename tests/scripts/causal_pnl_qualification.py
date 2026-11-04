"""Separate strict source-identifier admission and full mixed-book time/cash path.
The original complete independent monetary answers remain exactly unchanged.
New full nine-product controls use actual economic-date normalization, original
cash/credit/Gaussian/carry owners in independent Python, eligible published
observations and all-member CTD selection. No native outputs are read.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
from pathlib import Path
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_ready as wire
from delivery_hedge_ready import wire as identifiers
ROOT=c.ROOT;FOLDER=c.FOLDER;SOURCES=['causal-pnl-bindings.json','causal-pnl-boundaries.json'];TARGET=FOLDER/'causal-pnl-qualified.json';clone=copy.deepcopy
old_native=a.native

def native(info,h):
 p=h['product'].get('product',h['product'])
 if p['kind']=='swap' and not p['terms']['trade_id'].startswith('lifecycle.synthetic.'):
  t=p['terms'];root=info['point']['valuation_date'];known=info['point']['known_at'];history={(r['index_id'],c.day(r['observation_date'])):D(r['rate']) for r in info['inputs']['fixings'] if r['known_at']<=known and r['published_at']<=known};z=c.curve_zero(info['inputs'],t['collateral']['discount_curve']['id']);projection=c.curve_zero(info['inputs'],t['floating']['projection_curve']['id']);value=c.g.swap_price(t,z,projection,history);pv=D(0);shift=(z*D((c.day(root)-c.g.REF).days)/365).exp()
  for leg in ['fixed','floating']:
   rows=[]
   for f in value[leg]:
    if f['payment']<=root:continue
    f=clone(f);f['pv']=D(f['pv'])*shift;rows.append(f);pv+=f['pv']
   value[leg]=rows
  value['pv']=pv;return dict(kind='swap',pv=pv,full=value)
 return old_native(info,h)

def generate():
 cases=[]
 for name in SOURCES:
  source=json.loads((FOLDER/name).read_bytes())
  for c0 in source['cases']:
   item=clone(c0);identifiers(item['request']);cases.append(item)
 with localcontext() as context:
  context.prec=60;before,cs=c.main_before();received=c.repoint(before,'mixed_cash.received');close='2026-10-09';c.set_contexts(received,'2026-10-07',c.instant(close));indexes={'USD.SOFR'}
  # Bind every actual original rate-future index, rather than a provider alias.
  for h in received['holdings']:
   p=h['product'].get('product',h['product'])
   if p['kind']=='rate_future':indexes.add(p['input']['contract']['settlement']['accrual']['index'])
  existing={(r['index_id'],r['observation_date'])for r in received['inputs']['fixings']}
  for p in received['inputs']['publications']:
   if p['index_id'] in indexes and '2026-10-07'<p['observation_date']<=close and p['expected_publication']<=c.instant(close) and (p['index_id'],p['observation_date']) not in existing:received['inputs']['fixings'].append(dict(index_id=p['index_id'],observation_date=p['observation_date'],rate='0.0385',published_at=p['expected_publication'],known_at=p['expected_publication'],revision=1,status='Final',source_id='CAUSAL.ACTUAL.ADMINISTRATOR'));existing.add((p['index_id'],p['observation_date']))
  for h in received['holdings']:
   p=h['product'].get('product',h['product'])
   if p['kind']=='rate_future':p['input']['fixing_stamp']['version']+=1
  rolled=c.repoint(received,'mixed_cash.rolled');c.set_contexts(rolled,close)
  for h in rolled['holdings']:
   p=h['product'].get('product',h['product'])
   if p['kind']=='rate_future':
    index=p['input']['contract']['settlement']['accrual']['index']
    for obs in received['inputs']['fixings'][len(before['inputs']['fixings']):]:
     if obs['index_id']==index:c.append_event(h,obs['observation_date'],'fixing',obs,known=c.instant(close))
   if p['kind']=='bond_future':
    for member in p['input']['members']:
     for funding in [member['financing'],member['own_cash'],member['coupon_carry'].get('rate')]:
      if funding:funding['start']=close;funding['stamp']['version']+=10
  paid=c.repoint(rolled,'mixed_cash.actual_receipts');c.payment(paid['holdings'][0],cs[0][0],cs[0][0]['amount']/2,known=c.instant(close),on=close)
  for claim in cs[2]:c.payment(paid['holdings'][2],claim,claim['amount']/2,known=c.instant(close),on=close)
  moved=c.market(paid,'mixed_cash.market_ctd');req=c.request(before,[('fixing',received),('time',rolled),('event',paid),('market',moved)],[None,None,None,c.market(before,'mixed_cash.market_single')],reporting='EUR');opening=c.ClaimSet(cs);opening._events=[len(h['lifecycle']['events']) for h in before['holdings']];old=a.native;a.native=native
  try:expected=a.financial(req,opening)
  finally:a.native=old
  cases.append(dict(id='causal.full.time_fixing_cash_ctd',request=identifiers(wire.encode(req)),expected_error=None,expected=expected))
 return dict(certified=False,reference='Strict original SourceId spelling admission only for prior controls. New separate complete nine-product time/fixing/cash/market path retains original independent financial owners, actual curves and all-member CTD switches. No native outputs.',source_reads={'tests/fixtures/library/'+name:hashlib.sha256((FOLDER/name).read_bytes()).hexdigest() for name in SOURCES},tolerances=json.loads((FOLDER/SOURCES[0]).read_bytes())['tolerances'],cases=cases)

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=c.de.serial)+'\n').encode()
 if args.write:
  if TARGET.exists():raise ValueError('Refusing to replace complete independent controls')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Full qualified-input declaration differs')
 print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
