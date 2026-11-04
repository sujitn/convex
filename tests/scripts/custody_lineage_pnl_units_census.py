"""Lossless complete active custody controls with original actual fill units."""
import argparse,json
import residual_pnl_census as original
original.SOURCE='custody-lineage-pnl-units.json'
source_cases=original.source_cases
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    parts=original.build();paths=[original.FOLDER/f'custody-lineage-pnl-units-api-{i+1:02d}.json' for i in range(len(parts))]
    if a.write and any(p.exists() for p in paths):raise ValueError('Refusing overwrite')
    for path,data in zip(paths,parts):
        if a.write:path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Complete active request and outcome differs')
    print(json.dumps(dict(status='passed',cases=len(source_cases()),parts=len(parts),certified=False)))
