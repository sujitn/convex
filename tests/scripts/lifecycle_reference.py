"""Independent exact synthetic earned/settled cash reference; no Convex execution."""
from pathlib import Path
from decimal import Decimal as D, localcontext, ROUND_HALF_EVEN, ROUND_HALF_UP
from datetime import date
import hashlib,json,sys
ROOT=Path(__file__).resolve().parents[2]
INPUT=ROOT/'tests/fixtures/lifecycle/inputs.json'
OUTPUT=INPUT.with_name('expected.json')
def days(a,b):return D((date.fromisoformat(b)-date.fromisoformat(a)).days)
def generate():
    data=json.loads(INPUT.read_text())
    with localcontext() as ctx:
        ctx.prec=60
        def cash(v):return str(v.quantize(D('.01'),rounding=ROUND_HALF_EVEN))
        # Original independent profile segments, actual elapsed calendar days.
        h1=D(1000)*(D('.04')*days('2024-01-02','2024-04-02')+D('.06')*days('2024-04-02','2024-07-02'))/D(365)
        h2=D(600)*D('.06')*days('2024-07-02','2025-01-02')/D(365)
        earned=D(1000)*(D('.04')*days('2024-01-02','2024-04-02')+D('.06')*days('2024-04-02','2024-04-15'))/D(365)
        factor=D(1)
        # Explicit original shifted weights Apr2,3,4,5,8; Friday spans3days.
        for weight in [1,1,1,3,1]:factor*=D(1)+D('.05')*D(weight)/D(360)
        # Initial step-in is exactly the June20 coupon roll: original accrued
        # refund is zero; the June19-20 scheduled premium remains its own claim.
        accrued=D(1000000)*D('.01')*(days('2024-06-20',data['default']['date'])+D(data['default']['accrual_day_fraction']))/D(360)
        # Independent normal settlement: rounded monetary levels, then signed
        # integer contracts. Official settlement precision differs from order ticks.
        opening = D('100')
        daily = D('99.970009')
        multiplier = D('2500')
        contracts = -3
        previous = (opening * multiplier).quantize(D('.01'), rounding=ROUND_HALF_UP)
        current = (daily * multiplier).quantize(D('.01'), rounding=ROUND_HALF_UP)
        variation = cash((current - previous) * D(contracts))
        result={'schema_version':1,'certified':False,'provenance':'Independent Python Decimal60; original synthetic inputs and stated formulas only. No native/candidate outputs read. Exact cents after declared rounding.','input_sha256':hashlib.sha256(INPUT.read_bytes()).hexdigest(),'bond':{'coupon_1':cash(h1),'coupon_2':cash(h2),'principal_1':'400.00','principal_2':'600.00','exercise_accrued':cash(earned),'exercise_cash':cash(D(1010)+D(cash(earned)))},'irs':{'fixed_1':cash(-D(1000000)*D('.04')*D(29)/D(360)),'float_1':cash(D(1000000)*(D('.05')*D('1.25')+D('.001'))*D(29)/D(360)),'fixed_2':cash(-D(500000)*D('.045')*D(31)/D(360)),'float_2':cash(D(500000)*(D('-.005')*D('1.25')+D('.001'))*D(31)/D(360))},'ois':{'fixed':cash(-D(1000000)*D('.04')*D(7)/D(360)),'float':cash(D(1000000)*(D('1.25')*(factor-D(1))+D('.001')*D(7)/D(360)))},'cds':{'initial_cash':cash(-D(1000000)*D('.02')+D(1000000)*D('.01')*days('2024-06-20','2024-06-20')/D(360)),'first_premium':cash(-D(1000000)*D('.01')*D(1)/D(360)),'default_protection':cash(D(1000000)*(D(1)-D(data['default']['recovery']))),'default_accrual':cash(-accrued)},'futures':{'opening_points':'100','daily_points':'99.970009','currency_per_point':'2500','contracts':-3,'previous_rounded_contract':cash(previous),'current_rounded_contract':cash(current),'variation':variation}}
    return (json.dumps(result,indent=2)+'\n').encode()
def main():
    content=generate()
    if sys.argv[1:] == ['write']:OUTPUT.write_bytes(content)
    elif sys.argv[1:] == ['check']:
        if OUTPUT.read_bytes()!=content:raise SystemExit('independent lifecycle reference differs')
    else:raise SystemExit('use write or check')
    print(json.dumps({'status':'passed','certified':False,'sha256':hashlib.sha256(content).hexdigest()}))
if __name__=='__main__':main()
