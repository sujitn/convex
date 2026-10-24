// Literal dated annual coupon cash and independent Decimal references.
const fs = require('node:fs');
const api = require(process.argv[2]);
const fixture = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
const fixed = {coupon_rate:5,maturity_date:'2028-06-15',issue_date:'2026-06-15',settlement_date:'2026-06-15',face_value:100,frequency:1,day_count:'30/360',currency:'USD'};
const points = [{date:'2027-06-15',rate:4},{date:'2028-06-15',rate:4},{date:'2030-06-15',rate:4}];
const observations = [], failures = [];
const bound = 1e-8;
for (const row of fixture.cases.filter(r => r.compounding !== 'Daily')) {
  const params = {...fixed,compounding:row.compounding};
  const rate = Number(row.rate), expected = Number(row.present_value);
  const price = api.price_from_yield(params, rate * 100);
  const analysis = api.analyze_bond(params, expected, points);
  const checks = {price:price.dirty_price,ytm:analysis.ytm,
    macaulay_duration:analysis.macaulay_duration,modified_duration:analysis.modified_duration,
    convexity:analysis.convexity,dv01:analysis.dv01};
  const references = {price:expected,ytm:rate*100,macaulay_duration:Number(row.macaulay_duration),
    modified_duration:Number(row.modified_duration),convexity:Number(row.convexity),dv01:Number(row.dv01_loss)};
  if (price.error || analysis.error) failures.push({id:row.id,price,analysis});
  for (const [field, actual] of Object.entries(checks)) {
    if (typeof actual !== 'number' || !Number.isFinite(actual) || Math.abs(actual-references[field]) > bound)
      failures.push({id:row.id,field,actual,expected:references[field],bound});
  }
  observations.push({id:row.id,params,price,analysis,checks,references});
}
// A direct same-pricer yield quote cannot accept context-free ICMA or ActualPeriod.
const contextFailures = [];
for (const update of [{day_count:'ACT/ACT ICMA'},{compounding:'ActualPeriod'},{market:'UK',instrument_type:'GovernmentBond'}]) {
  const params={...fixed,...update};
  const price=api.price_from_yield(params,4), analysis=api.analyze_bond(params,100,points);
  if (!price.error || !analysis.error) failures.push({context:update,price,analysis});
  contextFailures.push({update,price,analysis});
}
const result={status:failures.length?'failed':'passed',absolute_bound:bound,observations,contextFailures,failures,certified:false};
fs.writeFileSync(process.argv[4],JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({status:result.status,cases:observations.length,context_controls:contextFailures.length,failures:failures.length,certified:false}));
if(failures.length)process.exitCode=1;
