"""Exact committed book selection and position binding; no financial arithmetic."""
import copy
import urllib.parse

import market_data_adapter as adapter
import lifecycle_risk
import lifecycle_history


class BookInputError(ValueError):
    """Fixed safe diagnostic for an incomplete or inconsistent book release."""


def resolve(client, release):
    """Use the existing durable commit read; never use latest mutable reference."""
    key = release['reference_commit']
    try:
        commit = client.call('GET', '/reference/commits/' + urllib.parse.quote(key, safe=''))
    except adapter.IngestionError as error:
        if error.http_status == 404:
            raise BookInputError('book_reference_not_committed') from None
        raise
    if commit['idempotency_key'] != key:
        raise BookInputError('book_commit_identity')
    pin = release['workflow']['plan']['definition']
    records = [r for r in commit.get('master_records', []) if r['definition']['kind'] == 'calculation'
               and r['definition']['definition']['id'] == pin['id'] and r['version'] == pin['version']]
    if len(records) != 1 or records[0]['recorded_at'] != commit['recorded_at']:
        raise BookInputError('book_definition_commit_mismatch')
    return records[0]


def bind(config, definition, position_step):
    """One full native P&L closing point supplies quantities, FX and hedge state.

    Verify every risk product/reference/curve binding against that same point.
    Full supplied fills and lifecycle facts stay with native P&L validation.
    """
    calculations = lifecycle_history.expand(definition)['calculations']
    pnl_spec = calculations[config['pnl']]
    if pnl_spec['request']['operation']['kind'] != 'book_pnl':
        raise BookInputError('book_pnl_required')
    data = pnl_spec['request']['operation']['input']
    if position_step != len(data['steps']) - 1:
        raise BookInputError('book_position_must_be_pnl_close')
    close = data['steps'][position_step]['after']
    point = close['point']
    positions = {p['identity']['id']: (p, i) for p, i in zip(point['positions'], close['holdings'], strict=True)}
    if len(positions) != len(point['positions']) or set(positions) != set(config['risks']):
        raise BookInputError('book_position_coverage')
    request = copy.deepcopy(config['hedge'])
    problem = request['operation']['problem']
    if problem['reporting_currency'] != data['reporting_currency']:
        raise BookInputError('book_reporting_currency')
    for identity, calc in config['risks'].items():
        position, index = positions[identity]
        holding = data['holdings'][index]
        spec = calculations[calc]
        operation = spec['request']['operation']
        reference = copy.deepcopy(position['current'])
        reference['quantity'] = reference['reference_quantity']
        if (holding['id'] != identity or holding['product']['kind'] != 'product'
                or operation['kind'] not in ('graph_hedge_risk', 'lifecycle_hedge_risk')
                or operation['product'] != holding['product']['product']
                or operation['input']['identity'] != position['identity']
                or operation['input']['reference_position'] != reference
                or spec['curve_sets'] != [pnl_spec['curve_sets'][close['market']]]
                or position['identity']['entity'] != problem['entity']):
            raise BookInputError('book_risk_position_mismatch')
        if operation['kind'] == 'lifecycle_hedge_risk':
            try:
                lifecycle_risk.check_binding(operation, holding, data)
            except (ValueError, KeyError, TypeError):
                raise BookInputError('lifecycle_risk_history_mismatch') from None
        elif any(s['event']['action']['kind'] in ('terminate', 'exercise_notice', 'exercise', 'credit_default', 'recovery', 'bond_delivery', 'security_receipt')
                 for s in holding.get('lifecycle', {}).get('events', [])):
            raise BookInputError('lifecycle_risk_required')
    for row in problem['holdings']:
        position, _ = positions[row['risk']]
        row['current_quantity'] = position['current']['quantity']
        row['pending_quantity'] = position['pending_quantity']
    for row in [*problem['holdings'], *problem['candidates']]:
        position, _ = positions[row['risk']]
        matches = [f for f in point['fx'] if f['from'] == position['current']['currency']
                   and f['to'] == data['reporting_currency']]
        if len(matches) != 1:
            raise BookInputError('book_fx_binding')
        row['fx'] = copy.deepcopy(matches[0])
    problem['position_state'] = copy.deepcopy(point['manifest'])
    problem['settlement'] = point['valuation_date']
    return request
