"""Lossless bounded API descriptors of complete declared mixed-book inputs."""
import argparse,hashlib,json
from pathlib import Path
from delivery_hedge_census import compact,spans
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
SOURCES=['mixed-scenario-publications.json','mixed-scenario-boundaries.json']
def source_cases():
 cases=[c for name in SOURCES for c in json.loads((FOLDER/name).read_bytes())['cases']]
 if len(cases)!=48 or len({c['id'] for c in cases})!=48:raise ValueError('Full declared corpus scope differs')
 return cases

def descriptor(source):
 decoder=json.JSONDecoder();cursor=1;fields={}
 while True:
  while source[cursor] in ' \t\r\n,':cursor+=1
  if source[cursor]=='}':break
  key,cursor=decoder.raw_decode(source,cursor)
  while source[cursor] in ' \t\r\n:':cursor+=1
  start=cursor;_,cursor=decoder.raw_decode(source,cursor)
  fields[key]=source[start:cursor]
 return '{'+','.join(json.dumps(key)+':'+fields[key] for key in ['id','request','expected_error'])+'}'

def build():
 original=[];reads={}
 for name in SOURCES:
  raw=(FOLDER/name).read_bytes();reads[name]=hashlib.sha256(raw).hexdigest();original.extend(descriptor(case) for case in spans(raw.decode(),'cases'))
 prefix=json.dumps(dict(schema_version=1,certified=False,source_sha256=reads,qualification='Lossless complete request/outcome descriptors. Original financial controls, settings and lexemes remain separate and immutable; native results are transport controls only.'))[:-1]+',"cases":['
 parts=[compact((prefix+','.join(original[i:i+4])+']}').encode()) for i in range(0,len(original),4)]
 expected=[{k:c[k] for k in ['id','request','expected_error']} for c in source_cases()]
 if [c for part in parts for c in json.loads(part)['cases']]!=expected:raise ValueError('Complete supplied requests changed')
 if any(len(part)>2*1024*1024 for part in parts):raise ValueError('Bounded census part exceeded')
 return parts
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();parts=build();paths=[FOLDER/f'mixed-scenario-api-{i+1:02d}.json' for i in range(len(parts))]
 if a.write and any(p.exists() for p in paths):raise ValueError('Refusing overwrite')
 for path,data in zip(paths,parts):
  if a.write:path.write_bytes(data)
  elif path.read_bytes()!=data:raise ValueError('Lossless census changed')
 print(json.dumps(dict(status='passed',cases=48,parts=len(parts),bytes=[len(v) for v in parts],certified=False)))
