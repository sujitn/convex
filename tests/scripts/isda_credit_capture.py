#!/usr/bin/env python3
"""Execute original ISDA 1.8.3 six-pillar calibration and 23 risk scenarios.

Requires a supplied checksum-matching publisher ZIP and clang/clang++. No network,
installation or production linkage. Raw compiler/runtime streams are retained.
"""
import argparse,datetime as dt,hashlib,json,subprocess,tempfile,zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
PACK=Path('tests/fixtures/comparisons/credit-isda-1.8.3')
SOURCE_SHA='bee02772ef45c3ff93de995a1655b3c5231aec4e257173bc4d21d47c362e0db0'
def sha(data):return hashlib.sha256(data).hexdigest()
def require(ok,message):
 if not ok:raise ValueError(message)
def capture(archive,output):
 require(sha(archive.read_bytes())==SOURCE_SHA,'publisher source ZIP differs')
 require(not output.exists(),'refusing to overwrite original model evidence')
 output.mkdir(parents=True);output=output.absolute()
 raw=(ROOT/PACK/'inputs.json').read_bytes();data=json.loads(raw)
 original=(ROOT/'tests/fixtures/comparisons/cds-isda-1.8.3/inputs.json').read_bytes()
 require(sha(original)==data['original_discount_inputs_sha256'],'original discount input bytes differ')
 require(data['discount_recipe']==json.loads(original)['discount_recipe'] and len(data['experiments'])==23,'original supplied discount/scenario scope differs')
 reference=dt.date.fromisoformat(data['discount_recipe']['reference_date'])
 (output/'nodes.txt').write_text(''.join(f"{(dt.date.fromisoformat(n['pillar']['Date'])-reference).days} {n['quote']['value']:.17g}\n" for n in data['discount_recipe']['source']['Nodes']))
 (output/'cases.txt').write_text(''.join(f"{e['id']} {int(e['hold_survival'])} {e['discount_shift_decimal']:.17g} {e['recovery_fraction']:.17g} {' '.join(format(q,'.17g') for q in e['running_spreads_decimal'])}\n" for e in data['experiments']))
 commands=[]
 def run(label,argv):
  started=dt.datetime.now(dt.timezone.utc).isoformat()
  with (output/(label+'.stdout')).open('wb') as stdout,(output/(label+'.stderr')).open('wb') as stderr:
   result=subprocess.run(list(map(str,argv)),cwd=ROOT,stdout=stdout,stderr=stderr,timeout=60)
  commands.append(dict(label=label,argv=list(map(str,argv)),started_at=started,finished_at=dt.datetime.now(dt.timezone.utc).isoformat(),exit_code=result.returncode,stdout_sha256=sha((output/(label+'.stdout')).read_bytes()),stderr_sha256=sha((output/(label+'.stderr')).read_bytes())))
  (output/'commands.json').write_text(json.dumps(commands,indent=2)+'\n')
  require(result.returncode==0,'original subprocess failed: '+label)
 with tempfile.TemporaryDirectory(prefix='convex-credit-isda183-') as tmp:
  build=Path(tmp)
  with zipfile.ZipFile(archive) as source:
   entries=source.infolist();require(len(entries)<=512 and sum(e.file_size for e in entries)<=32*1024*1024,'publisher ZIP work bound exceeded')
   require(len({e.filename for e in entries})==len(entries),'duplicate source ZIP member')
   for e in entries:require(not Path(e.filename).is_absolute() and '..' not in Path(e.filename).parts,'invalid ZIP path')
   source.extractall(build);model=build/'isda_cds_model_c_v1.8.3'
   for e in entries:
    if not e.is_dir():require((build/e.filename).read_bytes()==source.read(e),'original source bytes changed')
  source_files={p.relative_to(model).as_posix():sha(p.read_bytes()) for p in sorted(model.rglob('*')) if p.is_file()}
  include=model/'lib/include/isda';objects=build/'objects';objects.mkdir()
  run('compiler',['clang','--version']);sources=sorted((model/'lib/src').glob('*.c'));require(len(sources)==48,'original compile scope differs')
  for i,source in enumerate(sources):run(f'compile-{i:02d}',['clang','-O2','-include','ldate.h','-DUNIX','-DLINUX','-DVERSION="1.8.3"','-I'+str(include),'-c',source,'-o',objects/(source.stem+'.o')])
  run('compile-cpp',['clang++','-O2','-DUNIX','-DLINUX','-DVERSION="1.8.3"','-I'+str(include),'-c',model/'lib/src/cfinanci.cpp','-o',objects/'cfinanci.o'])
  driver=ROOT/'tests/scripts/isda_credit_capture.c'
  run('compile-driver',['clang','-O2','-I'+str(include),'-c',driver,'-o',objects/'driver.o'])
  binary=build/'capture';run('link',['clang++',*[objects/(s.stem+'.o') for s in sources],objects/'cfinanci.o',objects/'driver.o','-lm','-o',binary]);binary_sha=sha(binary.read_bytes())
  run('execution',[binary,output/'nodes.txt',output/'cases.txt',ROOT/'tests/fixtures/comparisons/cds-isda-1.8.3/NYM.csv'])
 actual=json.loads((output/'execution.stdout').read_bytes())
 require(actual['model_version']=='CDS version "1.8.3"' and [e['id'] for e in actual['experiments']]==[e['id'] for e in data['experiments']],'executed model/scope differs')
 (output/'manifest.json').write_text(json.dumps(dict(model_version=actual['model_version'],source_archive_sha256=SOURCE_SHA,source_files=source_files,driver_sha256=sha(driver.read_bytes()),input_sha256=sha(raw),capture_tool_sha256=sha(Path(__file__).read_bytes()),binary_sha256=binary_sha,output_sha256=sha((output/'execution.stdout').read_bytes()),commands=53,executed=True,certified=False),indent=2)+'\n')
 return dict(status='original_model_executed',experiments=len(actual['experiments']),valuations=sum(len(e['valuations']) for e in actual['experiments']),commands=len(commands),certified=False)
if __name__=='__main__':
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--source-archive',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
 try:print(json.dumps(capture(args.source_archive,args.output)))
 except (ValueError,OSError,subprocess.TimeoutExpired) as e:print(json.dumps(dict(status='execution_failed',error=str(e),certified=False)));raise SystemExit(1)
