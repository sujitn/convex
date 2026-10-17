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


def checked_state(result, report):
    """Require exact native state/claims/amounts, not an approximate scalar match."""
    state = result['lifecycle']
    identity = result['vector']['id']
    closing = len(report['input']['causes'])
    point = report['input']['points'][closing]
    position = next(p for p in point['positions'] if p['identity']['id'] == identity)
    mark = next(t for t in report['trades'] if t['id'] == identity)['marks'][closing]
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
