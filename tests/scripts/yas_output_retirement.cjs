// Actual generated wasm-bindgen output; no browser/server or prototype replacement.
const fs = require('node:fs');
const modulePath = process.argv[2], outputPath = process.argv[3];
if (fs.existsSync(outputPath)) throw new Error('Refuse actual control overwrite');
const wasm = require(modulePath);
const fixed = {coupon_rate:5,maturity_date:'2030-06-15',issue_date:'2020-06-15',
  settlement_date:'2026-04-20',face_value:100,frequency:2,day_count:'30/360',currency:'USD'};
const points = [{date:'2027-06-15',rate:4},{date:'2028-06-15',rate:4.1},
  {date:'2030-06-15',rate:4.2},{date:'2032-06-15',rate:4.3}];
const results = [], failures = [];
for (const price of [95,100,105]) {
  const actual = wasm.analyze_bond(fixed,price,points);
  if (actual.error || actual.clean_price !== price || !Number.isFinite(actual.dv01))
    failures.push({price,reason:'retained fixed calculation failed',actual});
  for (const field of ['asw_spread','oas','option_value','effective_duration','effective_convexity','is_callable'])
    if (Object.hasOwn(actual,field)) failures.push({price,field,reason:'retired field advertised'});
  results.push({price,actual});
}
const result = {status:failures.length?'failed':'passed',modulePath,results,failures,certified:false};
fs.writeFileSync(outputPath,JSON.stringify(result,null,2)+'\n',{flag:'wx'});
process.stdout.write(JSON.stringify({status:result.status,prices:results.length,failures:failures.length})+'\n');
if (failures.length) process.exitCode=1;
