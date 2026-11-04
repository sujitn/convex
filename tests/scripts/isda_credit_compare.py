#!/usr/bin/env python3
"""Offline actual original ISDA multi-pillar credit and current-native comparison.

Standard library only; no model installation/network. Frozen compiler/source
execution and the new native process are separate, explicitly qualified evidence.
"""
import argparse,datetime as dt,json,subprocess
from decimal import Decimal
from pathlib import Path
import isda_cds_compare as base
ROOT=base.ROOT;PACK=Path('tests/fixtures/comparisons/credit-isda-1.8.3')
PINS={
 "tests/fixtures/comparisons/credit-isda-1.8.3/compiler.stdout": "f6a41663474c412322e565ec1a1aa27bcb42151b98d527dc401b7d744b9a4237",
 "tests/fixtures/comparisons/credit-isda-1.8.3/inputs.json": "a2e30428a157f58afe75728e8014a64a48e746f2295dfcf9801ca0b69fd3cd75",
 "tests/fixtures/comparisons/credit-isda-1.8.3/isda-commands.json": "562f5877a604bc6a4186b0072b6083cee62cc9d7b207f6473316b1b5e6e97d30",
 "tests/fixtures/comparisons/credit-isda-1.8.3/isda-manifest.json": "39d12eb6019be6a3fd5a90cdc7a8e00f76df379884f98a00835c6c19593c610d",
 "tests/fixtures/comparisons/credit-isda-1.8.3/isda-reference.json": "b95e98b84b85531eda23070a2849a6da3a04ae57929f5e45b931aa0192a2ee2d",
 "tests/fixtures/comparisons/credit-isda-1.8.3/isda-reference.stderr": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
 "tests/fixtures/comparisons/credit-isda-1.8.3/mixed-inputs.json": "bac57e44771c869aa734d0b2bbeea8de7e374afba79feb8ea3fe0920a4e06153",
 "tests/scripts/isda_credit_capture.c": "0fba540ff9012af254426d3469142d6c589ef39739f57fb1cb3738d371b35088",
 "tests/scripts/isda_credit_capture.py": "beeadb2552058aa55cb1917e4c6b39f44d693b215d645021bd3487e06a01668e"
}
BOUNDS={'hazard_absolute':1e-9,'survival_absolute':1e-10,'quote_basis_points_absolute':1e-7,'upfront_fraction_absolute':1e-10,'pv_currency_absolute':0.001,'risk_currency_absolute':0.1,'risk_relative':1e-6}
require=base.require;loads=base.loads;number=base.number;digest=base.digest
def reference(root=ROOT):
 base.reference(root)
 for path,pin in PINS.items():require(digest((root/path).read_bytes())==pin,'original credit bytes differ: '+path)
 data=loads((root/PACK/'inputs.json').read_bytes());captured=loads((root/PACK/'isda-reference.json').read_bytes());manifest=loads((root/PACK/'isda-manifest.json').read_bytes());commands=loads((root/PACK/'isda-commands.json').read_bytes())
 require(data['bounds']==BOUNDS and data['model_version']=='1.8.3' and len(data['experiments'])==23 and len(data['pillars'])==6,'predeclared credit scope/bounds differ')
 original=loads((root/base.PACK/'inputs.json').read_bytes());source=loads((root/base.PACK/'isda-manifest.json').read_bytes())
 require(data['original_discount_inputs_sha256']==base.PINS['inputs.json'] and data['discount_recipe']==original['discount_recipe'] and data['template']==original['template'] and data['calendar_holidays']==original['calendar_holidays'],'original dates/calendars/discount dependencies differ')
 require(manifest['source_archive_sha256']==base.SOURCE_SHA and manifest['source_files']==source['source_files'] and manifest['source_files']['license.txt']==base.LICENSE_SHA and manifest['executed'] is True and manifest['certified'] is False and manifest['commands']==53,'executed original source/model/license provenance differs')
 require(manifest['driver_sha256']==PINS['tests/scripts/isda_credit_capture.c'] and manifest['capture_tool_sha256']==PINS['tests/scripts/isda_credit_capture.py'] and manifest['input_sha256']==PINS[(PACK/'inputs.json').as_posix()] and manifest['output_sha256']==PINS[(PACK/'isda-reference.json').as_posix()],'actual driver/input/output binding differs')
 require(len(commands)==53 and commands[0]['label']=='compiler' and commands[-1]['label']=='execution','original command scope differs')
 previous=None
 for command in commands:
  start,finish=(dt.datetime.fromisoformat(command[k]) for k in ['started_at','finished_at'])
  require(command['exit_code']==0 and command['argv'] and start.tzinfo is not None and finish>=start and (previous is None or start>=previous),'original command failed or clocks differ');previous=finish
 require(commands[0]['stdout_sha256']==PINS[(PACK/'compiler.stdout').as_posix()] and commands[-1]['stdout_sha256']==manifest['output_sha256'] and commands[-1]['stderr_sha256']==PINS[(PACK/'isda-reference.stderr').as_posix()],'raw original compiler/runtime binding differs')
 require(captured['model_version']==manifest['model_version']=='CDS version "1.8.3"' and [e['id'] for e in captured['experiments']]==[e['id'] for e in data['experiments']],'original model/scenario ordering differs')
 mixed=loads((root/PACK/'mixed-inputs.json').read_bytes());require(mixed['source_output_sha256']==manifest['output_sha256'] and len(mixed['helpers'])==6,'actual original upfront quote input provenance differs')
 for m in mixed['helpers']:
  v=next(v for v in captured['experiments'][0]['valuations'] if v['pillar_index']==m['pillar_index'] and number(v['coupon_decimal'])==number(m['coupon_decimal']))
  if m['quote_kind']!='RunningSpread':require(number(m['quote']['value'])==number(v['clean_upfront_fraction' if m['quote_kind']=='CleanUpfront' else 'dirty_upfront_fraction']),'mixed quote differs from original actual engine output')
 return data,captured,mixed
def validate_candidate(candidate,data,captured,mixed):
 require(type(candidate) is dict and set(candidate)=={'schema_version','engine','model_version','notice','experiments','additional_fits','sensitivities'} and candidate['schema_version']==1 and candidate['engine']=='Convex' and candidate['model_version']=='1.8.3' and candidate['notice']==base.NOTICE,'candidate identity/model/notice differs')
 require(len(candidate['experiments'])==23 and [e['id'] for e in candidate['experiments']]==[e['id'] for e in data['experiments']],'missing/reordered native scenarios')
 errors=[];maxima={k:Decimal(0) for k in ['hazard','survival','upfront','pv','risk']};count=0
 def check(actual,expected,kind,bound,relative=0):
  nonlocal count
  error=abs(number(actual)-number(expected));maxima[kind]=max(maxima[kind],error);count+=1
  if error>number(bound)+number(relative)*abs(number(expected)):errors.append(dict(measure=kind,actual=str(actual),expected=str(expected),absolute_error=str(error),absolute_bound=str(bound),relative_bound=str(relative)))
 def fit(report,expected,source_input,mixed_mode=False,compare_parameters=True):
  require(type(report) is dict and set(report)=={'survival_probabilities','hazards_per_act365f_year','repricing','recipe','price_evaluations','global'},'native fit fields differ')
  require(len(report['repricing'])==6 and len(report['survival_probabilities'])==len(report['hazards_per_act365f_year'])==6 and type(report['price_evaluations']) is int and 0<report['price_evaluations']<=8192,'native fit dimensions/work differ')
  recipe=report['recipe'];require(recipe['pillars']==data['pillars'] and len(recipe['helpers'])==6 and recipe['curve']['source']=={'Helpers':[f'CDS.Pillar.{i}' for i in range(6)]},'original full helper/pillar scope differs')
  require(recipe['curve']['role']=={'kind':'Credit','entity_id':data['template']['reference']['entity_id'],'seniority':'senior_unsecured','recovery':source_input['recovery_fraction']} and recipe['curve']['currency']=='USD' and recipe['curve']['representation']=='SurvivalProbabilities' and recipe['curve']['time_basis']=='Act365Fixed' and recipe['curve']['interpolation']=='LogLinear' and recipe['curve']['extrapolation']=='None','credit role/model methods differ')
  if 'CreditGlobal' in recipe['curve']['calibration']:
   require(report['global'] is not None and report['global']['rank']==6 and 0<=report['global']['iterations']<=100 and number(report['global']['scaled_rms'])<=Decimal('1e-9'),'actual global identification/convergence differs')
  else:require(recipe['curve']['calibration']=='Piecewise' and report['global'] is None,'actual calibration mode differs')
  for i,(h,q) in enumerate(zip(recipe['helpers'],report['repricing'])):
   require(h['id']==q['helper_id']==f'CDS.Pillar.{i}' and h['quote_version']==100+i and h['pillar']==q['pillar']==data['pillars'][i] and h['context']=={'valuation_date':'2022-06-21','step_in_date':'2022-06-22','cash_settlement_date':'2022-06-24','initial_payment_state':'NotContracted'},'original native quote/version/date lineage differs')
   unit=['DecimalRate','BasisPoints','PercentRate'][i%3];scale={'DecimalRate':1,'BasisPoints':10000,'PercentRate':100}[unit]
   expected_quote={'value':source_input['running_spreads_decimal'][i]*scale,'unit':unit};tolerance=[1e-11,1e-7,1e-9][i%3];coupon='0.01';kind='RunningSpread'
   if mixed_mode:
    m=mixed['helpers'][i];expected_quote=m['quote'];tolerance=m['tolerance'];coupon=m['coupon_decimal'];kind=m['quote_kind']
   require(h['quote_kind']==kind and h['terms']['coupon_decimal']==coupon and h['quote']['unit']==expected_quote['unit'] and abs(number(h['quote']['value'])-number(expected_quote['value']))<=Decimal('1e-12') and h['tolerance']==q['tolerance']==tolerance and q['input']==h['quote'] and q['model']['unit']==q['input']['unit'],'native quote/coupon/unit/tolerance changed')
   require(number(q['residual'])==number(q['model']['value']-q['input']['value']) and abs(number(q['residual']))<=number(tolerance),'actual native residual failed')
   for name in ['entity_id','seniority','document','restructuring_clause']:
    require(h['terms']['reference'][name]==data['template']['reference'][name],'original reference lineage differs')
   require(h['terms']['reference']['recovery_fraction']==source_input['recovery_fraction'] and h['terms']['premium_calendar']==data['template']['premium_calendar'] and h['terms']['settlement_calendar']==data['template']['settlement_calendar'] and h['terms']['survival_curve']==recipe['curve']['key'],'original recovery/calendar/curve input differs')
  for actual,expected_value in zip(report['survival_probabilities'],expected['survival_probabilities']):
   if compare_parameters:check(actual,expected_value,'survival',BOUNDS['survival_absolute'])
   else:require(abs(number(actual)-number(expected_value))<=number(BOUNDS['survival_absolute']),'fit survival differs from original')
  for actual,expected_value in zip(report['hazards_per_act365f_year'],expected['hazards_per_act365f_year']):
   if compare_parameters:check(actual,expected_value,'hazard',BOUNDS['hazard_absolute'])
   else:require(abs(number(actual)-number(expected_value))<=number(BOUNDS['hazard_absolute']),'fit hazard differs from original')
 for native,original,source in zip(candidate['experiments'],captured['experiments'],data['experiments']):
  require(set(native)=={'id','survival_probabilities','hazards_per_act365f_year','valuations','calibration','recovery_fraction','discount_shift_decimal','hold_survival','running_spreads_decimal'},'native scenario fields differ')
  require(all(native[k]==source[k] for k in ['id','recovery_fraction','discount_shift_decimal','hold_survival','running_spreads_decimal']),'scenario rates/quotes/recovery axes differ')
  require(len(native['survival_probabilities'])==len(native['hazards_per_act365f_year'])==6 and len(native['valuations'])==12,'native numerical scope differs')
  for n,o in zip(native['survival_probabilities'],original['survival_probabilities']):check(n,o,'survival',BOUNDS['survival_absolute'])
  for n,o in zip(native['hazards_per_act365f_year'],original['hazards_per_act365f_year']):check(n,o,'hazard',BOUNDS['hazard_absolute'])
  for n,o in zip(native['valuations'],original['valuations']):
   require(set(n)==set(o) and n['pillar_index']==o['pillar_index'] and n['coupon_decimal']==o['coupon_decimal'],'native trade order/coupon units differ')
   for name in ['clean_upfront_fraction','dirty_upfront_fraction']:check(n[name],o[name],'upfront',BOUNDS['upfront_fraction_absolute'])
   check(n['net_pv'],o['net_pv'],'pv',BOUNDS['pv_currency_absolute'])
  if source['hold_survival']:require(native['calibration'] is None,'held hazard mislabeled as an actually recalibrated curve')
  else:fit(native['calibration'],original,source,compare_parameters=False)
 require([f['id'] for f in candidate['additional_fits']]==['running_global','mixed_piecewise','mixed_global'],'missing full global/mixed-unit fits')
 for f in candidate['additional_fits']:fit(f['fit'],captured['experiments'][0],data['experiments'][0],f['id'].startswith('mixed'))
 expected_ids=['rates_hold','recovery_hold','rates_recalibrate','recovery_recalibrate']+[f'quote_{i}' for i in range(6)]
 require([s['id'] for s in candidate['sensitivities']]==expected_ids,'missing/reordered signed risk')
 by_id={e['id']:e for e in captured['experiments']}
 for s in candidate['sensitivities']:
  plus=next(v for v in by_id[s['id']+'_plus']['valuations'] if v['pillar_index']==5 and v['coupon_decimal']==0.01)['net_pv'];minus=next(v for v in by_id[s['id']+'_minus']['valuations'] if v['pillar_index']==5 and v['coupon_decimal']==0.01)['net_pv']
  expected=(number(plus)-number(minus))/(Decimal('.02') if s['id'].startswith('recovery') else 2)
  kind='RecoveryDerivative' if s['id'].startswith('recovery') else 'Pv01Signed' if s['id'].startswith('rates') else 'Cs01Signed'
  require(s['measure']==kind and s['factor']['family']==('recovery' if kind=='RecoveryDerivative' else 'interest_rate' if kind=='Pv01Signed' else 'credit_spread'),'risk family/monetary units differ')
  check(s['value'],str(expected),'risk',BOUNDS['risk_currency_absolute'],BOUNDS['risk_relative'])
 return dict(status='passed' if not errors else 'numerical_mismatch',experiments=23,valuations=276,compared_values=count,max_absolute_errors={k:str(v) for k,v in maxima.items()},errors=errors,certified=False)
def offline(binary,report):
 data,original,mixed=reference();binary=binary.resolve(strict=True);report=report.absolute();report.parent.mkdir(parents=True,exist_ok=True)
 stdout=report.with_name(report.stem+'-native.stdout');stderr=report.with_name(report.stem+'-native.stderr');require(not any(p.exists() for p in [report,stdout,stderr]),'refusing to overwrite comparison evidence')
 result=dict(status='execution_incomplete',certified=False,original_pins=PINS,binary=str(binary),binary_sha256=digest(binary.read_bytes()))
 paths=['Cargo.toml','Cargo.lock','crates/convex-analytics/Cargo.toml','crates/convex-analytics/src/credit_calibration.rs','crates/convex-analytics/src/credit_risk.rs','crates/convex-curves/src/contracts/build.rs','crates/convex-curves/src/contracts/credit_fit.rs','crates/convex-curves/src/contracts/inputs.rs','crates/convex-curves/src/contracts/query.rs','crates/convex-analytics/tests/credit_support/mod.rs','crates/convex-analytics/examples/credit_comparison.rs','tests/scripts/isda_credit_compare.py']+[p.relative_to(ROOT).as_posix() for p in sorted((ROOT/'crates/convex-credit/src').glob('*.rs'))]
 result['source_pins']={p:digest((ROOT/p).read_bytes()) for p in paths};started=dt.datetime.now(dt.timezone.utc).isoformat()
 try:
  with stdout.open('wb') as out,stderr.open('wb') as err:run=subprocess.run([str(binary)],cwd=ROOT,stdout=out,stderr=err,timeout=60)
  result['command']=dict(argv=[str(binary)],started_at=started,finished_at=dt.datetime.now(dt.timezone.utc).isoformat(),exit_code=run.returncode,stdout_sha256=digest(stdout.read_bytes()),stderr_sha256=digest(stderr.read_bytes()))
  require(run.returncode==0,'actual native exporter failed');result['native']=validate_candidate(loads(stdout.read_bytes()),data,original,mixed);result['status']=result['native']['status']
 except (ValueError,KeyError,TypeError,OSError,subprocess.TimeoutExpired) as e:result['error']=str(e)
 report.write_text(json.dumps(result,indent=2,allow_nan=False)+'\n');return result
if __name__=='__main__':
 parser=argparse.ArgumentParser(description=__doc__);sub=parser.add_subparsers(dest='mode',required=True);sub.add_parser('check');run=sub.add_parser('offline');run.add_argument('--candidate-binary',type=Path,required=True);run.add_argument('--report',type=Path,required=True);args=parser.parse_args()
 try:
  if args.mode=='check':reference();result=dict(status='original_credit_capture_checked',experiments=23,commands=53,certified=False)
  else:result=offline(args.candidate_binary,args.report)
  small={k:v for k,v in result.items() if k not in ['source_pins','original_pins']};print(json.dumps(small));raise SystemExit(0 if result['status'] in ['passed','original_credit_capture_checked'] else 1)
 except (ValueError,KeyError,TypeError,OSError) as e:print(json.dumps(dict(status='rejected',error=str(e),certified=False)));raise SystemExit(1)
