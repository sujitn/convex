#!/usr/bin/env python3
"""Audit the complete required surface map; this is never a certificate."""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
INVENTORY = 'tools/library/library-inventory.json'
MATRIX = 'tools/library/api-capabilities.json'
SCOPE_FIELDS = ('requirement_areas', 'conventions', 'models', 'combinations',
                'records', 'outside_or_inapplicable')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON key: ' + key)
        result[key] = value
    return result


def read(path):
    def nonfinite(value):
        raise ValueError('nonfinite JSON value: ' + value)
    content = path.read_bytes()
    require(len(content) <= 8 * 1024 * 1024, 'capability input exceeds 8 MiB')
    return json.loads(content, object_pairs_hook=unique_object, parse_constant=nonfinite)


def scope_hash(inventory):
    scope = {key: inventory[key] for key in SCOPE_FIELDS}
    return hashlib.sha256(json.dumps(scope, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def check_routes(routes, examples, root):
    require(set(routes) == {f'LC-{i:03d}' for i in range(1, 31)}, 'required capability family omitted')
    for identity, route in routes.items():
        require(route['qualification'].strip(), 'route needs exact limits')
        require(route['anchors'], 'route needs source anchors')
        require(route['entry_points'] == {'rust': 'convex::library::execute',
                'shared_json': 'convex::library::execute_json', 'c_abi': 'convex_library'},
                'required native/JSON/C entry point changed')
        for anchor in route['anchors']:
            path = Path(anchor['path'])
            require(not path.is_absolute() and '..' not in path.parts, 'unsafe source anchor')
            path = (root / path).resolve()
            require(path.is_relative_to(root.resolve()) and path.is_file(), 'missing source: ' + identity)
            require(anchor['text'] in path.read_text(), 'missing dispatch anchor: ' + identity)
        for kind in ('positive_examples', 'negative_examples'):
            require(route[kind], 'route needs actual positive and negative examples: ' + identity)
            for identifier in route[kind]:
                require(identifier in examples, 'missing actual example: ' + identifier)
                require((examples[identifier]['expected_error'] is None) == (kind == 'positive_examples'),
                        'wrong example outcome: ' + identifier)


def check_operation_routes(routes, examples, required, root):
    require(isinstance(routes, dict) and routes, 'exact operation routes omitted')
    for identity, route in routes.items():
        require(route['certified'] is False and route['qualification'].strip(),
                'operation examples cannot certify financial coverage: ' + identity)
        require(route['required_record_ids'] and set(route['required_record_ids']) <= required,
                'operation moved or waived original required record: ' + identity)
        require(len(set(route['required_record_ids'])) == len(route['required_record_ids']),
                'duplicate required operation record: ' + identity)
        require(route['entry_points'] == {'rust': 'convex::library::execute',
                'shared_json': 'convex::library::execute_json', 'c_abi': 'convex_library'},
                'exact operation needs native/JSON/C reachability')
        require(route['selectors'], 'exact operation needs actual request selector')
        for anchor in route['anchors']:
            path = Path(anchor['path'])
            require(not path.is_absolute() and '..' not in path.parts, 'unsafe operation source')
            path = (root / path).resolve()
            require(path.is_relative_to(root.resolve()) and path.is_file()
                    and anchor['text'] in path.read_text(), 'missing native operation source: ' + identity)
        require(route['anchors'], 'exact operation needs native and dispatcher anchors')
        for kind in ('positive_examples', 'boundary_examples', 'negative_examples'):
            require(route[kind] and len(set(route[kind])) == len(route[kind]),
                    'missing or duplicate exact operation example: ' + identity)
            for identifier in route[kind]:
                require(identifier in examples, 'missing exact operation example: ' + identifier)
                example = examples[identifier]
                require((example['expected_error'] is None) == (kind != 'negative_examples'),
                        'wrong operation example outcome: ' + identifier)
                for selector, expected in route['selectors'].items():
                    actual = example['request']
                    try:
                        for part in selector.split('.'):
                            actual = actual[int(part)] if isinstance(actual, list) else actual[part]
                    except (KeyError, IndexError, ValueError, TypeError) as error:
                        raise ValueError('operation selector absent: ' + identifier) from error
                    require(actual == expected, 'example calls another operation: ' + identifier)


def check(matrix=None, root=ROOT):
    inventory = read(root / INVENTORY)
    matrix = matrix if matrix is not None else read(root / MATRIX)
    require(type(matrix['schema_version']) is int and matrix['schema_version'] == 1, 'unknown report schema')
    require(matrix['certified'] is False and type(matrix['verified_records']) is int
            and matrix['verified_records'] == 0, 'an adapter inventory cannot certify the library')
    require(set(matrix['clients']) == {'java', 'excel', 'mcp', 'wasm'}, 'advertised client omitted')
    for client in matrix['clients'].values():
        require(client['qualification'].strip(), 'host qualification missing')
    require(matrix['mandatory_scope_sha256'] == scope_hash(inventory), 'required LB0/LB1 scope changed')
    required = {record['id'] for record in inventory['records']}
    actual = {record['record_id'] for record in matrix['records']}
    require(required == actual and len(actual) == len(matrix['records']), 'mandatory record missing or duplicated')
    cases = read(root / 'tests/fixtures/library/cases.json')['cases']
    supplemental = matrix['supplemental_corpora']
    require(supplemental and len(set(supplemental)) == len(supplemental), 'operation corpus omitted or duplicated')
    for name in supplemental:
        path = Path(name)
        require(not path.is_absolute() and '..' not in path.parts and path.suffix == '.json'
                and path.parent == Path('tests/fixtures/library'), 'unsafe supplemental operation corpus')
        document = read(root / path)
        require(document['schema_version'] == 1 and document['certified'] is False,
                'input corpus cannot certify finance')
        cases += document['cases']
    examples = {case['id']: case for case in cases}
    require(len(examples) == len(cases), 'duplicate example identity')
    check_routes(matrix['routes'], examples, root)
    check_operation_routes(matrix['operation_routes'], examples, required, root)
    by_id = {record['id']: record for record in inventory['records']}
    for record in matrix['records']:
        require(record['certificate_status'] == 'Planned', 'engineering route cannot promote original mandatory status')
        original = by_id[record['record_id']]
        require(record['surface'] == original['surface'], 'required surface changed')
        require(record['route_id'] == record['record_id'].split('/')[0], 'record moved to another capability family')
        require(record['route_id'] in matrix['routes'], 'record needs actual route or explicit gap context')
        require(record['dispatch_status'] in ('candidate', 'partial', 'unsupported', 'compatibility_unverified'),
                'unknown capability')
        require(record['qualification'].strip(), 'record needs a reason')
        if record['dispatch_status'] == 'unsupported':
            require(record['remaining_owner_tasks'], 'unsupported required operation needs an owner')
    return {'status': 'passed', 'mandatory_records': len(actual), 'verified_records': 0, 'certified': False,
            'exact_operation_routes': len(matrix['operation_routes']), 'engineering_examples': len(cases),
            'unsupported_records': sum(record['dispatch_status'] == 'unsupported' for record in matrix['records']),
            'qualification': 'Actual route examples are engineering subsets; every exact convention/model record remains Planned.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', action='store_true')
    parser.parse_args()
    print(json.dumps(check(), indent=2))


if __name__ == '__main__':
    main()
