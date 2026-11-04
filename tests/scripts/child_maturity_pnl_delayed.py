"""Separate complete maturity books with an explicit two-calendar-day cash delay.

The retained initial books inherit zero contractual payment delay; May8 in2029
is an actual business day, unlike the original2027 Sunday coupon boundary.
Supply a complete new two-day fixed-bond cash convention consistently in every
original product/lifecycle/delivery security copy before deriving financial cash.
The previous declaration and failed additional delayed-DF assertion remain.
No preceding input, financial answer, pricing body or tolerance changes. Actual
payments remain on May10 and mature model exposure stays ended. No candidate
answer is read. All16 original full regression books remain unchanged.
"""
import argparse, hashlib, json
import child_maturity_pnl_reference as original
from decimal import localcontext
SOURCE = original.TARGET
TARGET = SOURCE.with_name('child-maturity-pnl-delayed.json')
expand_original = original.states.expand

def expand(request):
    book = expand_original(request)
    def bind(value):
        if isinstance(value, dict):
            if 'face_value' in value and 'profiles' in value and 'schedule' in value and 'dates' in value:
                value['dates']['payment_delay'] = {'CalendarDays': 2}
            for child in value.values():bind(child)
        elif isinstance(value, list):
            for child in value:bind(child)
    bind(book)
    return book

def generate():
    before = original.states.expand
    original.states.expand = expand
    try:
        data = original.generate()
    finally:
        original.states.expand = before
    data.update(reference=__doc__,initial_trial_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    data=generate();raw=(json.dumps(data,indent=2,default=original.r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Full separately supplied maturity cash-delay declaration differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
