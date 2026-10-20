"""Bind exhausted malformed acquisitions/transfers to the actual final cutoff.

Every financial book and amount remains identical to the original complete
settlement declaration. A fully paid native obligation's original ordered receipt
prefix is available at its final actual date/known-at. The initial malformed
trial retained an earlier cutoff; this separate input repair exercises exhaustion
after full admissible paid-prefix facts rather than an earlier cutoff failure.
"""
import argparse
import hashlib
import json
import settled_claims_pnl_reference as original

SOURCE = original.TARGET
TARGET = SOURCE.with_name('settled-claims-pnl-admitted.json')


def generate():
    data = original.generate()
    for case in data['cases']:
        if case['id'] not in ['settled.invalid.exhausted_acquisition',
                              'settled.invalid.exhausted_transfer']:
            continue
        spec = case['request']['operation']['input']
        point = spec['book']['steps'][case['closure_point'] - 1]['after']['point']
        cutoff = (spec['claim_transfers'][-1]['cutoff'] if case['id'].endswith('transfer')
                  else spec['dated_entitlements'][0]['residual']['cutoff'])
        cutoff.update(date=point['valuation_date'], known_at=point['known_at'])
    data.update(reference=__doc__, original_trial_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    return data


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    data = generate()
    raw = (json.dumps(data, indent=2, default=original.r.c.de.serial) + '\n').encode()
    if args.write:
        if TARGET.exists():
            raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw:
        raise ValueError('Complete admitted settlement controls differ')
    print(json.dumps(dict(status='passed', cases=len(data['cases']),
        financial_books=sum(c['expected_error'] is None for c in data['cases']), certified=False)))
