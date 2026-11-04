"""Lossless lifecycle prefix sharing for existing managed core checkpoints.

No balances, claims or financial calculations are restored here. The server
expands these exact facts before calling the unchanged native lifecycle owners.
"""
import copy
import json


def lifecycles(definition):
    for spec in definition['calculations'].values():
        operation = spec['request']['operation']
        if operation['kind'] == 'book_pnl':
            for holding in operation['input']['holdings']:
                yield holding['lifecycle']
        elif operation['kind'] == 'lifecycle_hedge_risk':
            yield operation['lifecycle']


def archive(definition):
    rows = definition.get('reference_inputs', {}).get('lifecycle_history', {})
    if (not isinstance(rows, dict) or len(rows) > 128
            or 'lifecycle_history' in definition.get('reference_inputs', {}) and not rows):
        raise ValueError('lifecycle_history_archive')
    for identity, life in rows.items():
        if (not isinstance(life, list) or not 1 <= len(life) <= 4096
                or any(s['event']['trade_id'] != identity for s in life)):
            raise ValueError('lifecycle_history_prefix')
    return rows


def expand(definition):
    """Return full original requests for existing admission and native parity."""
    value = copy.deepcopy(definition)
    rows = archive(value)
    if not rows:
        return value
    for life in lifecycles(value):
        selection = life['events']
        if isinstance(selection, list):
            continue
        if (not isinstance(selection, dict) or set(selection) != {'prefix', 'tail'}
                or not isinstance(selection['prefix'], str) or not isinstance(selection['tail'], list)
                or selection['prefix'] not in rows or selection['prefix'] != life['trade_id']):
            raise ValueError('lifecycle_history_selection')
        original = rows[selection['prefix']]
        if len(original) + len(selection['tail']) > 4096:
            raise ValueError('lifecycle_history_opening_or_bound')
        life['events'] = copy.deepcopy(original + selection['tail'])
    return value


def pack(definition):
    """Share existing exact prefixes; later events remain explicit tails."""
    value = expand(definition)
    rows = archive(value)
    if not rows:
        return value
    for life in lifecycles(value):
        original = rows.get(life['trade_id'])
        if original is not None and life['events'][:len(original)] == original:
            life['events'] = dict(prefix=life['trade_id'], tail=life['events'][len(original):])
    return value


def compact(definition, pnl='pnl'):
    """Flatten the prior archive and share only this new window's exact opening.

    Old immutable references/snapshots/results remain the audit record. This
    replacement is self-contained and requires no predecessor during recovery.
    """
    value = expand(definition)
    if 'pnl_checkpoint' not in value['reference_inputs']:
        raise ValueError('lifecycle_history_checkpoint_required')
    data = value['calculations'][pnl]['request']['operation']['input']
    rows = {}
    for index in data['before']['holdings']:
        life = data['holdings'][index]['lifecycle']
        if life['events']:
            identity = life['trade_id']
            if identity in rows and rows[identity] != life['events']:
                raise ValueError('lifecycle_history_multiple_origins')
            rows[identity] = copy.deepcopy(life['events'])
    value['reference_inputs'].pop('lifecycle_history', None)
    def size(item):
        return len(json.dumps(item, ensure_ascii=False, allow_nan=False, separators=(',', ':')).encode('utf-8'))
    for identity, prefix in list(rows.items()):
        count = sum(life['trade_id'] == identity and life['events'][:len(prefix)] == prefix
                    for life in lifecycles(value))
        if count * size(prefix) <= size({identity: prefix}) + count * size(dict(prefix=identity, tail=[])):
            del rows[identity]
    if rows:
        value['reference_inputs']['lifecycle_history'] = rows
    packed = pack(value)
    # Sharing a unique short history can cost more bytes. Keep the original
    # representation unless this checkpoint actually benefits from sharing.
    full = copy.deepcopy(value)
    full['reference_inputs'].pop('lifecycle_history', None)
    if size(packed) >= size(full):
        return full
    return packed
