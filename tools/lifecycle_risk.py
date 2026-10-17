"""Bind native lifecycle risk to the same causal P&L closing state; no math."""
import copy


def history(data):
    """Losslessly assemble the existing native quantity/fill/FX declaration."""
    points = [data['before']['point'], *[s['after']['point'] for s in data['steps']]]
    standalone = []
    for row in data['standalone']:
        if row['kind'] == 'available':
            standalone.append(dict(kind='available', point=len(points)))
            points.append(row['information']['point'])
        else:
            standalone.append(copy.deepcopy(row))
    return copy.deepcopy(dict(causes=[s['cause'] for s in data['steps']], points=points,
        standalone=standalone, reporting_currency=data['reporting_currency'],
        max_points=data['max_points'], max_product_evaluations=data['max_product_evaluations']))


def bind_requests(definition, config, identities):
    """Producer helper: full native inputs, using existing named calculations."""
    value = copy.deepcopy(definition)
    data = value['calculations'][config['pnl']]['request']['operation']['input']
    close = data['steps'][-1]['after']
    for identity in identities:
        holding = next(data['holdings'][i] for i in close['holdings'] if data['holdings'][i]['id'] == identity)
        operation = value['calculations'][config['risks'][identity]]['request']['operation']
        operation.update(kind='lifecycle_hedge_risk', lifecycle=copy.deepcopy(holding['lifecycle']),
            history=history(data), max_state_evaluations=5 * len(operation['input']['factors']))
    return value


def check_binding(operation, holding, data):
    if (operation['lifecycle'] != holding['lifecycle'] or operation['history'] != history(data)
            or type(operation['max_state_evaluations']) is not int
            or not 5 * len(operation['input']['factors']) <= operation['max_state_evaluations'] <= 80):
        raise ValueError('lifecycle_risk_history_mismatch')


def default_scope(operation):
    """Actual legal mapping, never inferred from a ticker or a curve name."""
    product = operation['product']
    if product['kind'] == 'cds':
        return product['terms']['reference']['entity_id'], 'actual_cds_recovery_unknown'
    instrument = operation['lifecycle']['instrument']
    if product['kind'] == 'bond' and instrument['kind'] == 'bond' and instrument.get('credit'):
        return instrument['credit']['entity'], 'actual_bond_recovery_unknown'
    raise ValueError('unpriced_default_mapping_missing')


def checked_state(result, report, operation=None):
    """Require exact native state/claims/amounts, not an approximate scalar match."""
    state = result['lifecycle']
    identity = result['vector']['id']
    closing = len(report['input']['causes'])
    point = report['input']['points'][closing]
    position = next(p for p in point['positions'] if p['identity']['id'] == identity)
    mark = next(t for t in report['trades'] if t['id'] == identity)['marks'][closing]
    expected_default = None
    if mark['state'].get('default_event') is not None:
        if operation is not None:
            expected_default = default_scope(operation)[0]
        elif result['vector']['product'] == 'cds':
            expected_default = result['vector']['credit_scope'][0]['entity_id']
        else:
            raise ValueError('lifecycle_default_mapping_missing')
    if state.get('default_entity') != expected_default:
        raise ValueError('lifecycle_default_scope_mismatch')
    matured = result.get('valuation') == 'Matured'
    if state.get('matured_instrument') != (position['identity']['instrument'] if matured else None):
        raise ValueError('lifecycle_maturity_scope_mismatch')
    # Native P&L has already checked the exact Decimal pending quantity. Its
    # legal zero spellings (for example 0.00) must not be reinterpreted here.
    if matured and any(s['base_pv'] != 0 or s['value'] != 0 for s in result['vector']['sensitivities']):
        raise ValueError('lifecycle_maturity_reference_mismatch')
    if (state['position'] != position or state['manifest'] != point['manifest']
            or state['settlement'] != point['valuation_date'] or state['snapshot_id'] != result['input']['snapshot_id']
            or result['lifecycle_mark']['state'] != mark['state'] or result['lifecycle_mark']['claims'] != mark['claims']
            or result['lifecycle_mark']['local'] != mark['local']):
        raise ValueError('lifecycle_risk_pnl_mismatch')
    for key in ('current', 'pending'):
        row = state[key]
        if (len(row) != len(result['vector']['sensitivities'])
                or any(s['snapshot_id'] != state['snapshot_id'] or s['base_pv'] != mark['local'][key]['value'] for s in row)):
            raise ValueError('lifecycle_risk_amount_mismatch')
    return copy.deepcopy(state)


def checked_pending(result, operation, data):
    """An auditable native default state has no invented recovery value or risk."""
    pending = result['lifecycle_unpriced']
    identity = operation['input']['identity']['id']
    point = data['steps'][-1]['after']['point']
    position = next(p for p in point['positions'] if p['identity']['id'] == identity)
    state = pending['state']
    entity, reason = default_scope(operation)
    defaults = [s['event'] for s in operation['lifecycle']['events'] if s['event']['action']['kind'] == 'credit_default']
    if (len(defaults) != 1 or 'lifecycle' in result or 'lifecycle_mark' in result
            or pending['position'] != position or pending['manifest'] != point['manifest']
            or pending['settlement'] != point['valuation_date']
            or pending['snapshot_id'] != result['input']['snapshot_id']
            or result['vector']['id'] != identity or state['trade_id'] != identity
            or pending['reason'] != reason or state['status'] != 'default_pending'
            or state['default_event'] != defaults[0]['event_id'] or state['recovery'] is not None
            or state['remaining_fraction'] != '0'
            or pending['default_entity'] != entity):
        raise ValueError('unpriced_default_binding_mismatch')
    return copy.deepcopy(pending)
