"""Lossless complete cohort controls with separately admitted full Money units."""
import argparse,json
import cohort_pnl_complete_census as c
from cohort_pnl_census import FOLDER,source_cases as earlier
SOURCE='cohort-pnl-full-wire.json'
def source_cases():
    cases=earlier()+json.loads((FOLDER/SOURCE).read_bytes())['cases']
    if len(cases)!=41 or len({x['id'] for x in cases})!=41:raise ValueError('Full admitted original cohort scope differs')
    return cases
def build():
    prior=c.SOURCE;c.SOURCE=SOURCE
    try:return c.build()
    finally:c.SOURCE=prior
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();parts=build();paths=[FOLDER/f'cohort-pnl-admitted-api-{i+1:02d}.json' for i in range(len(parts))]
    if a.write and any(p.exists() for p in paths):raise ValueError('Refusing overwrite')
    for path,data in zip(paths,parts):
        if a.write:path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Lossless full admitted requests differ')
    print(json.dumps(dict(status='passed',cases=41,parts=len(parts),certified=False)))
