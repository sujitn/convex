"""Bind explicit captured issuer reference records; no pricing or issuer inference."""
import copy


def captured(saved):
    content = saved['content']
    definition = content['definition']['definition']['definition']
    pin = definition.get('reference_inputs', {}).get('issuer_scope')
    record = content.get('issuer_scope')
    if pin is None and record is None:
        return None
    if (pin is None or record is None or record['definition']['kind'] != 'issuer_scope'
            or record['version'] != pin['version'] or record['definition']['definition']['id'] != pin['id']):
        raise ValueError('captured_issuer_scope_mismatch')
    return dict(reference=copy.deepcopy(pin), snapshot_id=saved['id'],
                directory=copy.deepcopy(record['definition']['definition']['directory']))


def owners(directory):
    issuers = {i['entity_id']: i for i in directory['issuers']}
    instruments = {i['id']: i for i in directory['instruments']}
    if (len(issuers) != len(directory['issuers']) or not 1 <= len(issuers) <= 16
            or len(instruments) != len(directory['instruments']) or not 1 <= len(instruments) <= 256
            or any(i['entity_id'] not in issuers for i in instruments.values())):
        raise ValueError('issuer_directory_coverage')
    return issuers, instruments


def validate_book(definition, config, directory):
    """Every affected closing contract supplies its own actual native default."""
    issuers, instruments = owners(directory)
    data = definition['calculations'][config['pnl']]['request']['operation']['input']
    close = data['steps'][-1]['after']
    for position, index in zip(close['point']['positions'], close['holdings'], strict=True):
        holding = data['holdings'][index]
        product = holding['product']['product']
        if product['kind'] not in ('bond', 'cds'):
            continue
        pin = position['identity']['instrument']
        mapping = instruments.get(pin['id'])
        if mapping is None or mapping['version'] != pin['version']:
            raise ValueError('issuer_security_mapping_missing')
        issuer = issuers[mapping['entity_id']]
        life = holding['lifecycle']
        entity = (product['terms']['reference']['entity_id'] if product['kind'] == 'cds'
                  else (life['instrument'].get('credit') or {}).get('entity'))
        if entity is not None and entity != issuer['entity_id']:
            raise ValueError('issuer_native_legal_mapping_mismatch')
        defaults = [s['event'] for s in life['events'] if s['event']['action']['kind'] == 'credit_default']
        notice = issuer['default']
        if notice is None:
            if defaults:
                raise ValueError('issuer_notice_missing')
            continue
        operation = definition['calculations'][config['risks'][position['identity']['id']]]['request']['operation']
        if (operation['kind'] != 'lifecycle_hedge_risk' or entity != issuer['entity_id'] or len(defaults) != 1
                or defaults[0]['event_id'] != notice['id'] or defaults[0]['trade_id'] != holding['id']
                or defaults[0]['effective_date'] != notice['effective_date'] or defaults[0]['known_at'] != notice['known_at']):
            raise ValueError('issuer_default_incomplete_contract_coverage')


def hedge(request, snapshot, scope, states):
    """Filter by the explicit source directory; native admission independently checks."""
    if scope['snapshot_id'] != snapshot:
        raise ValueError('issuer_scope_snapshot_mismatch')
    issuers, instruments = owners(scope['directory'])
    problem = request['operation']['problem']
    def defaulted(vector):
        if vector['product'] not in ('cash_bond', 'cds'):
            return False
        pin = vector['instrument']
        mapping = instruments.get(pin['id'])
        if mapping is None or mapping['version'] != pin['version']:
            raise ValueError('issuer_candidate_mapping_missing')
        return issuers[mapping['entity_id']]['default'] is not None
    problem['candidates'] = [c for c in problem['candidates'] if not defaulted(c['risk'])]
    request['operation']['action'] = dict(kind='solve_issuer_lifecycle', states=copy.deepcopy(states), scope=copy.deepcopy(scope))
    return request
