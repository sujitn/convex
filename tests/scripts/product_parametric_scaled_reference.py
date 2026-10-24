"""Independent original product input declaration with explicit parameter units.

This new contract revision supplies parameter scales, keeping all earlier quotes,
starts, native bounds, objective, tolerances and work ceilings byte-equivalent.
Earlier unscaled attempts and both prior declarations are retained unchanged.
"""
import argparse
import json
import product_parametric_admitted_reference as admitted

DEST=admitted.original.ROOT/'tests/fixtures/curves/product-parametric-scaled.json'

def build():
    fixture=admitted.build()
    for case in fixture['cases']:
        graph=case['inputs']['calibration']
        recipe=(graph['rates'] or graph['cash'])[0]['recipe']
        settings=recipe['curve']['calibration']['Parametric']
        settings['parameter_scales']=([.01,.01,.01,10.0] if settings['initial']['kind']=='nelson_siegel'
                                      else [.01,.01,.01,.01,10.0,10.0])
    fixture['parameter_units']='Explicit native loading/decay scales; graph Jacobian and condition diagnostics use divided solver coordinates; original native quotes/postconditions/work remain.'
    return fixture

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--check',action='store_true');args=parser.parse_args()
    content=json.dumps(build(),indent=2)+'\n'
    if args.check:
        if DEST.read_text()!=content:raise SystemExit('Frozen scaled control bytes differ')
        print('12 full product graphs /60 independent Decimal controls; explicit native parameter scales passed')
    else:
        if DEST.exists():raise SystemExit('Refusing to overwrite independent scaled declaration')
        DEST.write_text(content)
