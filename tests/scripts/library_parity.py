#!/usr/bin/env python3
"""Fresh native transport parity, never an independent pricing reference."""
import argparse,ctypes,hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
def require(condition,message):
    if not condition:raise ValueError(message)
def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def prepare(binary,output):
    content=subprocess.check_output([str(Path(binary).resolve()),'--emit-parity'])
    data=json.loads(content);original=json.loads((ROOT/'tests/fixtures/library/cases.json').read_bytes())
    require(data['certified'] is False,'transport parity cannot certify finance')
    require([(c['id'],c['request'],c['expected_error']) for c in data['cases']]==[(c['id'],c['request'],c['expected_error']) for c in original['cases']],'source cases changed; review them before refreshing')
    data['native_binary_sha256']=sha(binary)
    out=Path(output);out.parent.mkdir(parents=True,exist_ok=True);out.write_text(json.dumps(data,separators=(',',':'))+'\n')
    print(json.dumps({'status':'passed','cases':len(data['cases']),'native_binary_sha256':data['native_binary_sha256'],'artifact':str(out),'sha256':sha(out),'certified':False}))
def compare(library,parity):
    lib=ctypes.CDLL(str(Path(library).resolve()));call=lib.convex_library;call.argtypes=[ctypes.c_char_p];call.restype=ctypes.c_void_p
    free=lib.convex_string_free;free.argtypes=[ctypes.c_void_p];free.restype=None
    data=json.loads(Path(parity).read_bytes())
    for case in data['cases']:
        request=json.dumps(case['request'],separators=(',',':'),ensure_ascii=False).encode()
        pointer=call(request);require(pointer,'null owned response')
        try:actual=json.loads(ctypes.string_at(pointer))
        finally:free(pointer)
        require(actual==case['expected_envelope'],case['id'])
    print(json.dumps({'status':'passed','cases':len(data['cases']),'shared_library_sha256':sha(library),'parity_sha256':sha(parity),'certified':False}))
def main():
    p=argparse.ArgumentParser();s=p.add_subparsers(dest='command',required=True)
    a=s.add_parser('prepare');a.add_argument('--binary',required=True);a.add_argument('--output',required=True)
    a=s.add_parser('compare');a.add_argument('--library',required=True);a.add_argument('--parity',required=True)
    a=p.parse_args()
    if a.command=='prepare':prepare(a.binary,a.output)
    else:compare(a.library,a.parity)
if __name__=='__main__':main()
