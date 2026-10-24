"""Separately declared correction of original complete swap resource bindings.
The admitted prototype accidentally put two dependency keys at the wrong DTO
level and omitted a calendar binding. Preserve it exactly. Bind the same full
original terms to the explicit supplied discount/projection and weekend-only
calendars used by the independent dated formulas. Monetary answers/bounds and
all other cases are unchanged. No native output is read.
"""
import argparse,copy,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];SOURCE=ROOT/'tests/fixtures/library/causal-pnl-admitted.json';TARGET=SOURCE.with_name('causal-pnl-bindings.json')
def generate():
 d=json.loads(SOURCE.read_bytes())
 for c in d['cases']:
  if c['id'] not in ['causal.time.irs_full_cash','causal.time.ois_full_cash']:continue
  inp=c['request']['operation']['input'];resources=inp['resources'];calendars={x['pin']['id'] for x in resources['calendars']}
  for h in inp['holdings']:
   for t in [h['product']['product']['terms'],h['lifecycle']['instrument']['terms']]:
    t['collateral']['discount_curve']=t.pop('discount_curve');t['floating']['projection_curve']=t.pop('projection_curve')
    pins=list(t['fixed']['leg']['calendars'].values())+list(t['floating']['leg']['calendars'].values())
    if t['floating']['kind']=='overnight':pins.append(t['floating']['observation_calendar'])
    else:pins.extend(t['floating']['resets'][k] for k in ['reset_calendar','fixing_calendar'])
    for p in pins:
     if p['id'] not in calendars:resources['calendars'].append(dict(pin=p,holidays=[],weekend_days=[5,6]));calendars.add(p['id'])
 d.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Separate supplied-term/calendar dependency binding repair over unchanged independent dated cash, monetary answers and bounds; no native execution.')
 return d
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing to replace original separately declared controls')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Complete bindings differ')
 print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
