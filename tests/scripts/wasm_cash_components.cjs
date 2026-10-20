const fs = require('node:fs');
const modulePath = process.argv[2], outputPath = process.argv[3];
if (fs.existsSync(outputPath)) throw new Error('Refuse actual control overwrite');
const wasm = require(modulePath);
const controls = [[100,2,2.5],[1000,2,25],[1000.625,2,25.015625],[1000.625,1,50.03125],[1000.625,4,12.5078125]];
const observations = [], failures = [];
for (const [face,frequency,coupon] of controls) {
  const params = {coupon_rate:5,maturity_date:'2030-06-15',issue_date:'2020-06-15',settlement_date:'2026-04-20',face_value:face,frequency,day_count:'30/360',currency:'USD'};
  const actual = wasm.get_cash_flows(params);
  if (!Array.isArray(actual) || !actual.length) { failures.push({face,frequency,reason:'actual native cash unavailable',actual}); continue; }
  for (let i=0;i<actual.length;i++) {
    const r=actual[i], terminal=i===actual.length-1;
    const expected={coupon_amount:coupon,principal_amount:terminal?face:0,fee_amount:0,amount:coupon+(terminal?face:0)};
    for (const [field,value] of Object.entries(expected))
      if (!Number.isFinite(r[field]) || Math.abs(r[field]-value)>1e-12) failures.push({face,frequency,i,field,expected:value,actual:r[field]});
    if (r.cf_type!==(terminal?'coupon_and_principal':'coupon')) failures.push({face,frequency,i,reason:'actual native type differs',actual:r.cf_type});
    if (Math.abs(r.amount-r.coupon_amount-r.principal_amount-r.fee_amount)>1e-12) failures.push({face,frequency,i,reason:'component sum differs'});
  }
  observations.push({face,frequency,coupon,actual});
}
const value={status:failures.length?'failed':'passed',modulePath,absolute_bound:1e-12,observations,failures,certified:false};
fs.writeFileSync(outputPath,JSON.stringify(value,null,2)+'\n',{flag:'wx'});
process.stdout.write(JSON.stringify({status:value.status,cases:observations.length,failures:failures.length})+'\n');
if(failures.length)process.exitCode=1;
