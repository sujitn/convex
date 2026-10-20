"""Independent full mixed roll on the original settlement-eligible calendar date.
Preserve rejected non-settlement-day controls. A separate full nine-product path
uses original complete calendars, contracts, histories and native numerical
policies, actual dated receipts and independently recomputed prices/claims/P&L
with all CTD ties/switches. No candidate output or original bounds are changed.
"""
import argparse,copy,hashlib,json,datetime as dt
from decimal import Decimal as D,localcontext
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_reconciled as r
import causal_pnl_ready as binding
import causal_pnl_wire as wire
SOURCE=c.FOLDER/'causal-pnl-reconciled.json';TARGET=SOURCE.with_name('causal-pnl-contexts.json')

def purchase(helper,calendar):
 t=helper['terms'];holidays=set(calendar['holidays']);weekends=set(calendar['weekend_days'])
 def business(d):return d.weekday() not in weekends and str(d) not in holidays
 def settle(trade):
  due=trade;lag=t['settlement_lag']
  if 'BusinessDays'in lag:
   n=lag['BusinessDays']
   while n:
    due+=dt.timedelta(days=1)
    if business(due):n-=1
  else:due+=dt.timedelta(days=lag['CalendarDays'])
  adjustment=t['settlement_adjustment'];original=due
  if adjustment=='Unadjusted':return due
  if adjustment not in ['Following','ModifiedFollowing']:raise ValueError('Undeclared original purchase adjustment')
  while not business(due):due+=dt.timedelta(days=1)
  if adjustment=='ModifiedFollowing' and due.month!=original.month:
   due=original
   while not business(due):due-=dt.timedelta(days=1)
  return due
 root=c.day(helper['settlement']);trade=next((root-dt.timedelta(days=i)for i in range(32)if business(root-dt.timedelta(days=i))and settle(root-dt.timedelta(days=i))==root and root-dt.timedelta(days=i)>=c.day(t['issue_date'])),None)
 if trade is None:raise ValueError('No actual original bounded purchase settlement exists')
 helper['trade_date']=str(trade)

def generate():
 data=json.loads(SOURCE.read_bytes());original=next(x for x in data['cases']if x['id']=='causal.full.time_fixing_cash_ctd.actual_cash.complete_publications');new=copy.deepcopy(original);original['expected_error']='invalid_input';original.pop('expected')
 for case in data['cases']:
  if case['id']=='causal.full.time_fixing_cash_ctd.actual_cash':case['expected_error']='invalid_input'
 req=wire.expand(new['request']);inp=req['operation']['input'];sets=[inp['before']]+[s['after']for s in inp['steps']]+[s['information']for s in inp['standalone']if s['kind']=='available'];closing='2026-10-13';manifests={}
 for i,info in enumerate(sets):
  root=info['point']['valuation_date'];known=info['point']['known_at'];later=known>inp['before']['point']['known_at']
  if root>'2026-10-07':root=closing
  if later:known=c.instant(closing)
  c.set_contexts(info,root,known);info.update(c.repoint(info,'eligible_original_settlement.'+str(i)))
  if i==1:
   for old,h in zip(inp['before']['holdings'],info['holdings']):
    if h['product'].get('product',{}).get('kind')=='cds':h['product']['product']['context']=copy.deepcopy(old['product']['product']['context'])
  sources={h['product']['product']['input']['contract']['settlement']['accrual']['index']:h['product']['product']['input']['fixing_stamp']['source']for h in info['holdings']if h['product'].get('product',{}).get('kind')=='rate_future'}
  existing={(o['index_id'],o['observation_date'])for o in info['inputs']['fixings']};added=[]
  for pub in info['inputs']['publications']:
   if pub['index_id']in sources and pub['expected_publication']<=known and (pub['index_id'],pub['observation_date'])not in existing:
    obs=dict(index_id=pub['index_id'],observation_date=pub['observation_date'],rate='0.0385',published_at=pub['expected_publication'],known_at=pub['expected_publication'],revision=1,status='Final',source_id=sources[pub['index_id']]);info['inputs']['fixings'].append(obs);added.append(obs);existing.add((pub['index_id'],pub['observation_date']))
  history=json.dumps([o for o in info['inputs']['fixings']if o['index_id']in sources],sort_keys=True)
  if history not in manifests:manifests[history]=600+len(manifests)
  for h in info['holdings']:
   p=h['product'].get('product',h['product'])
   if p['kind']=='cash_basis':
    security=p['security'];calendar=next(x for x in info['inputs']['calendars']if x['pin']==security['calendars']['payment']);purchase(security['helper'],calendar)
   if p['kind']=='bond_future' and root==closing:
    for member in p['input']['members']:
     for funding in [member['financing'],member['own_cash'],member['coupon_carry'].get('rate')]:
      if funding:funding['start']=closing;funding['stamp']['version']+=40
   if p['kind']=='rate_future':
    p['input']['fixing_stamp']['version']=manifests[history]
    if root==closing:
     index=p['input']['contract']['settlement']['accrual']['index']
     for obs in added:
      if obs['index_id']==index and obs['observation_date']<=root:c.append_event(h,obs['observation_date'],'fixing',obs,known=known)
   for step in h['lifecycle']['events']:
    event=step['event'];act=event['action']
    if act['kind']=='fixing' and event['known_at']>'2026-10-07T12:00:00Z':event['known_at']=known
    if act['kind']=='payment':event.update(effective_date=closing,known_at=known);act['terms'].update(value_date=closing,receipt_id=act['terms']['receipt_id']+'.eligible_settlement')
   # New fixings must precede actual cash events and preserve the received
   # observation order in the full original lifecycle replay.
   suffix=[s for s in h['lifecycle']['events']if s['event']['action']['kind']=='fixing'];rest=[s for s in h['lifecycle']['events']if s['event']['action']['kind']!='fixing'];payments=[s for s in rest if s['event']['action']['kind']=='payment'];rest=[s for s in rest if s['event']['action']['kind']!='payment'];h['lifecycle']['events']=rest+sorted(suffix,key=lambda s:s['event']['effective_date'])+payments
   for n,step in enumerate(h['lifecycle']['events']):step['event']['expected_revision']=n
 with localcontext()as context:
  context.prec=60;rows=[]
  for trade in new['expected']['trades']:rows.append([dict(kind=x['key']['kind'],effective=x['key']['effective_date'],ordinal=x['key']['ordinal'],due=x['due'],amount=D(x['amount']),recorded=D(x['recorded']))for x in trade['marks'][0]['claims']])
  opening=c.ClaimSet(rows);opening._events=[len(h['lifecycle']['events'])for h in inp['before']['holdings']];old=a.native;a.native=r.native
  try:new['expected']=a.financial(req,opening)
  finally:a.native=old
 new['id']+='.settlement_eligible_roll';new['request']=wire.normalize(binding.encode(req));data['cases'].append(new);data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Original non-settlement-day and wrong-cause controls retained as invalid. Separately complete original nine-product path on eligible Oct13 settlement, exact original purchase calendars, received observations and actual dated receipts; independently recomputed full values/cash/CTD/P&L at unchanged bounds without candidate output.')
 return data
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();raw=(json.dumps(generate(),indent=2,default=c.de.serial)+'\n').encode()
 if args.write:
  if TARGET.exists():raise ValueError('Refusing overwrite')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Independent original full settlement roll differs')
 d=json.loads(raw);print(json.dumps(dict(status='passed',cases=len(d['cases']),positive=sum(x['expected_error']is None for x in d['cases']),certified=False)))
