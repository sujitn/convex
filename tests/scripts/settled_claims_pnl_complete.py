"""Complete exhausted-transfer origin/key identity with its original final prefix.

Staggered acquisition declares successor claims before first-child claims. Select
the original transfer matching that declared cash origin/key; list index zero can
refer to another original claim. Preserve all financial controls and all preceding
complete trial bytes; this separate repair changes only the exhausted malformed
transfer's original proposal/entitlement/residual/party binding.
"""
import argparse
import hashlib
import json
import settled_claims_pnl_admitted as original

SOURCE = original.TARGET
TARGET = SOURCE.with_name('settled-claims-pnl-complete.json')


def generate():
    data = original.generate()
    for case in data['cases']:
        if case['id'] != 'settled.invalid.exhausted_transfer':
            continue
        spec = case['request']['operation']['input']
        fact = spec['dated_entitlements'][0]
        current = spec['claim_transfers'][-1]
        matched = next(t for t in spec['claim_transfers'][:-1]
                       if t['origin'] == fact['origin'] and t['key'] == fact['key'])
        for field in ['origin', 'key', 'proposal_id', 'entitlement_source',
                      'residual_id', 'from', 'to']:
            current[field] = original.original.clone(matched[field])
    data.update(reference=__doc__, admitted_trial_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    return data


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    data = generate()
    raw = (json.dumps(data, indent=2, default=original.original.r.c.de.serial) + '\n').encode()
    if args.write:
        if TARGET.exists():
            raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw:
        raise ValueError('Complete exhausted original claim control differs')
    print(json.dumps(dict(status='passed', cases=len(data['cases']),
        financial_books=sum(c['expected_error'] is None for c in data['cases']), certified=False)))
