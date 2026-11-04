// Pre-edit actual JS semantic admission and per100 invariance controls.
const fs=require('node:fs');
const api=require(process.argv[2]);
const fixed={coupon_rate:5.0,maturity_date:'2030-06-15',issue_date:'2020-06-15',settlement_date:'2026-06-15',face_value:100,frequency:2,day_count:'30/360',currency:'USD',first_coupon_date:null,market:null,instrument_type:null,yield_convention:null,compounding:null,settlement_days:null,ex_dividend_days:null,use_business_days:null};
const points=[{date:'2027-06-15',rate:4.0},{date:'2028-06-15',rate:4.1},{date:'2030-06-15',rate:4.2},{date:'2032-06-15',rate:4.3}];
const calls=[['analyze_bond',p=>api.analyze_bond(p,100,points)],['price_from_yield',p=>api.price_from_yield(p,5,points)],['price_from_spread',p=>api.price_from_spread(p,25,points)],['get_cash_flows',p=>api.get_cash_flows(p)],['calculate_accrued',p=>api.calculate_accrued(p)],['calculate_simple_metrics',p=>api.calculate_simple_metrics(p,100)]];
const invalid=[['currency','XXUSD'],['market','MOON'],['frequency',3],['day_count','ACT/MOON'],['instrument_type','UNKNOWN'],['yield_convention','UNKNOWN'],['compounding','UNKNOWN'],['coupon_rate',NaN],['face_value',NaN],['face_value',1e-100],['instrument_type','CorporateFRN']];
const observations=[];let failures=0;
for(const [name,call] of calls){for(const [field,value] of invalid){const actual=call({...fixed,[field]:value});const rejected=!!actual.error;observations.push({name,field,value:String(value),actual,rejected});if(!rejected)failures++;}}
const faces=[];const faceInput={...fixed,settlement_date:'2026-04-20'};
const quoteCalls=calls.filter(([name])=>name!=='get_cash_flows');
for(const [name,call] of quoteCalls){const base=call(faceInput);for(const face of [100,1000,1000.625]){const actual=call({...faceInput,face_value:face});faces.push({name,face,actual});if(JSON.stringify(actual)!==JSON.stringify(base))failures++;}}
const baseCash=api.get_cash_flows(faceInput);const cashFaces=[];
for(const face of [100,1000,1000.625]){const actual=api.get_cash_flows({...faceInput,face_value:face});if(!Array.isArray(actual)||actual.length!==baseCash.length)failures++;else for(let i=0;i<actual.length;i++){if(actual[i].date!==baseCash[i].date||actual[i].cf_type!==baseCash[i].cf_type||Math.abs(actual[i].amount-baseCash[i].amount*face/100)>1e-12)failures++;}cashFaces.push({face,actual});}
const override=api.analyze_bond({...fixed,market:'US',instrument_type:'GovernmentBond',compounding:'Annual'},100,points);if(override.error||override.compounding_method!=='Annual')failures++;
const inputDomains=[];
for(const value of [NaN,Infinity,-Infinity,0,-1]){for(const [name,call] of [['analyze_bond',()=>api.analyze_bond(fixed,value,points)],['calculate_simple_metrics',()=>api.calculate_simple_metrics(fixed,value)]]){const actual=call();if(!actual.error)failures++;inputDomains.push({name,value:String(value),actual});}}
for(const value of [NaN,Infinity,-Infinity]){for(const [name,call] of [['price_from_yield',()=>api.price_from_yield(fixed,value)],['price_from_spread',()=>api.price_from_spread(fixed,value,points)]]){const actual=call();if(!actual.error)failures++;inputDomains.push({name,value:String(value),actual});}}
const invalidCurves=[];
for(const malformed of [[{date:'2028-06-15',rate:4,volatility:1}],null,[],[{date:'2028-06-15',rate:NaN}],[{date:'2025-06-15',rate:4}]]){for(const [name,call] of [['analyze_bond',()=>api.analyze_bond(fixed,100,malformed)],['price_from_spread',()=>api.price_from_spread(fixed,25,malformed)]]){const actual=call();if(!actual.error)failures++;invalidCurves.push({name,actual});}}
fs.writeFileSync(process.argv[3],JSON.stringify({status:failures?'failed':'passed',failures,observations,faces,cashFaces,override,inputDomains,invalidCurves,certified:false},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({status:failures?'failed':'passed',failures,negative_controls:observations.length,face_controls:faces.length,override_controls:1,cash_face_controls:cashFaces.length,numeric_domain_controls:inputDomains.length,curve_controls:invalidCurves.length,certified:false}));
if(failures)process.exitCode=1;
