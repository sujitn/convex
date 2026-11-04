"""Explicit bounded book profile shared by the existing orchestration adapters."""
import json
import market_data_adapter as adapter

PROFILE = 'book_128'


def expanded(definition):
    reference = definition.get('reference_inputs', {})
    if 'core_capacity' not in reference:
        return False
    value = reference['core_capacity']
    if value != PROFILE:
        raise ValueError('unknown_core_capacity_profile')
    return True


def payload_bytes(value):
    """Bound full book batches/commands; ordinary market records keep old bounds."""
    if not isinstance(value, dict):
        raise ValueError('book_payload_object')
    definitions = [value.get('definition', {})]
    definitions += [c.get('definition', {}).get('definition', {}) for c in value.get('master_changes', [])
                    if c.get('definition', {}).get('kind') == 'calculation']
    if not any(expanded(d) for d in definitions if isinstance(d, dict)):
        return adapter.payload_bytes(value)
    pending, count = [(value, 0)], 0
    while pending:
        child, depth = pending.pop()
        count += 1
        if count > 1_048_576 or depth > 64:
            raise ValueError('book_payload_bound')
        if isinstance(child, dict):
            pending.extend((v, depth+1) for v in child.values())
        elif isinstance(child, list):
            pending.extend((v, depth+1) for v in child)
    raw = json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(',', ':')).encode()
    if len(raw) > 16 * 1024 * 1024:
        raise ValueError('book_payload_bound')
    return raw
