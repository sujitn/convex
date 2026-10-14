#!/usr/bin/env python3
"""Enumerate every outstanding required record without inventing certification.

This joins the original mandatory dimensions to explicit operation mappings.
Family examples and native/host build success cannot fill an unmapped record.
Use the record IDs as coding-harness inputs; certificates retain their own owners.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path

import api

ROOT = Path(__file__).resolve().parents[2]


def report(root=ROOT):
    inventory = api.read(root / api.INVENTORY)
    matrix = api.read(root / api.MATRIX)
    api.check(matrix=matrix, root=root)
    combinations = {row['id']: row for row in inventory['combinations']}
    areas = {row['id']: row for row in inventory['requirement_areas']}
    assessed = {row['record_id']: row for row in matrix['records']}
    mapped = defaultdict(list)
    native = api.native_routes(root, inventory)
    for identity, operation in {**matrix['operation_routes'], **native}.items():
        for record in operation['required_record_ids']:
            mapped[record].append(identity)
    records = []
    for required in inventory['records']:
        identity = required['id']
        combination = combinations[required['combination_id']]
        assessment = assessed[identity]
        status = assessment['dispatch_status']
        routes = sorted(mapped[identity])
        if status == 'unsupported':
            gap = 'native_or_surface_implementation_required'
        elif status == 'compatibility_unverified':
            gap = 'actual_advertised_host_qualification_required'
        elif status == 'partial':
            gap = 'complete_economics_or_dispatch_required'
        elif not routes:
            gap = 'exact_operation_evidence_required'
        else:
            gap = 'full_combination_qualification_required'
        observations = [inventory['observations'][key]
                        for key in combination['observation_ids']]
        records.append(dict(record_id=identity, area_id=combination['area_id'],
            product=combination['product'], convention=combination['convention_id'],
            model=combination['model_id'], economic_terms=combination['economic_terms'],
            operation=required['operation'], surface=required['surface'],
            mandatory=True, certificate_status=required['status'],
            engineering_dispatch_status=status, exact_operation_routes=routes,
            remaining_work=gap, pending_implementation_tasks=required['pending_task_ids'],
            qualification_owner_tasks=assessment['remaining_owner_tasks'],
            qualification=assessment['qualification'],
            observed_gaps=sorted({g for o in observations for g in o['gaps']}),
            build_profiles=combination['build_profiles']))
    if len({r['record_id'] for r in records}) != len(inventory['records']):
        raise ValueError('A required record was lost or duplicated')
    area_rows = []
    for identity, area in areas.items():
        rows = [r for r in records if r['area_id'] == identity]
        area_rows.append(dict(area_id=identity, family=area['family'],
            mandatory_records=len(rows), exact_mapped_records=sum(bool(r['exact_operation_routes']) for r in rows),
            outstanding_by_kind=dict(sorted(Counter(r['remaining_work'] for r in rows).items())),
            unmapped_operations=sorted({r['operation'] for r in rows if not r['exact_operation_routes']})))
    return dict(schema_version=1, task='CVX-063', accepted_task=False, certified=False,
        mandatory_scope_sha256=api.scope_hash(inventory),
        summary=dict(mandatory_areas=len(areas), mandatory_combinations=len(combinations),
            mandatory_records=len(records), exact_operation_routes=len(matrix['operation_routes']),
            exact_native_operation_routes=len(native),
            exact_mapped_records=sum(bool(r['exact_operation_routes']) for r in records),
            unmapped_records=sum(not r['exact_operation_routes'] for r in records),
            engineering_status=dict(sorted(Counter(r['engineering_dispatch_status'] for r in records).items())),
            outstanding_by_kind=dict(sorted(Counter(r['remaining_work'] for r in records).items()))),
        areas=area_rows, records=records, clients=matrix['clients'],
        qualification='Exact engineering mappings are supplied subsets. Every mandatory convention/model/operation/surface record and independent certificate remains open until its own complete evidence exists.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--json', action='store_true', help='Emit all original record IDs and their remaining work')
    parser.add_argument('--area', choices=[f'LC-{i:03d}' for i in range(1, 31)])
    args = parser.parse_args()
    data = report()
    if args.area:
        data['records'] = [row for row in data['records'] if row['area_id'] == args.area]
        data['areas'] = [row for row in data['areas'] if row['area_id'] == args.area]
    if not args.json:
        data.pop('records')
        data.pop('clients')
    print(json.dumps(data, indent=2))


if __name__ == '__main__':
    main()
