"""Lossless bounded API descriptors for complete independently declared inputs."""
import argparse,hashlib,json
from pathlib import Path
from delivery_hedge_census import compact,spans
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library';SOURCE='causal-pnl-economic-cutoffs.json'
def source_cases():
 cases=json.loads((FOLDER/SOURCE).read_bytes())['cases']
 if len(cases)!=68 or len({c['id'] for c in cases})!=68:raise ValueError('Complete predeclared causal corpus changed')
 return cases

def descriptor(source):
 decoder=json.JSONDecoder();cursor=1;fields={}
 while True:
  while source[cursor] in ' \t\r\n,':cursor+=1
  if source[cursor]=='}':break
  key,cursor=decoder.raw_decode(source,cursor)
  while source[cursor] in ' \t\r\n:':cursor+=1
  start=cursor;_,cursor=decoder.raw_decode(source,cursor);fields[key]=source[start:cursor]
 return '{'+','.join(json.dumps(k)+':'+fields[k]for k in ['id','request','expected_error'])+'}'

def build():
 raw=(FOLDER/SOURCE).read_bytes();original=[descriptor(c)for c in spans(raw.decode(),'cases')];prefix=json.dumps(dict(schema_version=1,certified=False,source_sha256=hashlib.sha256(raw).hexdigest(),qualification='Lossless complete request/outcome descriptors; full independent monetary controls and retained original failures remain separate. Native envelopes prove transport only.'))[:-1]+',"cases":['
 # Three whole requests per part stay within the existing2MiB corpus-file limit.
 parts=[compact((prefix+','.join(original[i:i+3])+']}').encode())for i in range(0,len(original),3)]
 expected=[{k:c[k]for k in ['id','request','expected_error']}for c in source_cases()]
 if [c for p in parts for c in json.loads(p)['cases']]!=expected:raise ValueError('Original complete requests changed')
 if any(len(p)>2*1024*1024 for p in parts):raise ValueError('Census file bound exceeded')
 return parts
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();parts=build();paths=[FOLDER/f'causal-pnl-v12-api-{i+1:02d}.json'for i in range(len(parts))]
 if a.write and any(p.exists()for p in paths):raise ValueError('Refusing overwrite')
 for path,data in zip(paths,parts):
  if a.write:path.write_bytes(data)
  elif path.read_bytes()!=data:raise ValueError('Lossless original census changed')
 print(json.dumps(dict(status='passed',cases=68,parts=len(parts),bytes=[len(p)for p in parts],certified=False)))
