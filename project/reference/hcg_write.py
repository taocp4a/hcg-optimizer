#!/usr/bin/env python3
"""Re-serialize a structure produced by hcg_parse.parse() back to .hcg bytes (v0x78)."""
import struct, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import hcg_parse as H

def wstr(s): return struct.pack('<i',len(s))+s.encode('utf-16le')

def w_safebmp(B, payload=None):
    if B['is_path']: payload=B['path'].encode('utf-16le')
    elif payload is None: raise ValueError('embedded payload must be supplied')
    return struct.pack('<8sqIQI',b'SafeBMP ',len(payload),0xffffffff if B['is_path'] else 0,B['stamp'],B['reserved'])+payload

def w_obj(O, ver):
    rec=bytearray(bytes.fromhex(O['rec_hex']))
    struct.pack_into('<ff',rec,0,*O['center']); struct.pack_into('<4f',rec,0xc,*O['rect']); struct.pack_into('<i',rec,0x1c,O['type'])
    arr=O.get('arr',[]); struct.pack_into('<i',rec,0xf0,len(arr)) if (ver>=0x76 and arr) else None
    out=bytes(rec)+struct.pack('<%di'%len(arr),*arr)
    sh=bytearray(bytes.fromhex(O['sub']['raw']))
    sh[0]=1 if 'image' in O else 0; sh[1]=1 if O.get('rich') else 0
    struct.pack_into('<6i',sh,4,len(O.get('points',[])),len(O.get('text','')),len(O.get('text2','')),len(O.get('path','')),len(O.get('images',[])),O['sub']['len4'])
    out+=bytes(sh)
    if 'image' in O: out+=w_safebmp(O['image'],O['image'].get('payload'))
    for B in O.get('images',[]): out+=w_safebmp(B)
    out+=struct.pack('<%di'%len(O['tail16']),*O['tail16'])
    for p in O.get('points',[]): out+=struct.pack('<3i',*p)
    for k in ('text','text2'):
        if O.get(k): out+=O[k].encode('utf-16le')
    if O.get('path'):
        out+=O['path'].encode('utf-16le')
        if ver>=0x6e: out+=struct.pack('<Q',O['path_stamp'])
    if O.get('rich'):
        R=O['rich']; out+=wstr(R['text']); n=0
        for run in R['runs']:
            for _ in range(run['n']): out+=bytes.fromhex(run['fmt']['raw'])+wstr(run['fmt']['font']); n+=1
        assert n==len(R['text'])
    return out

def w_layer(L, ver):
    h=bytearray(bytes.fromhex(L['hdr_hex'])); struct.pack_into('<i',h,0,len(L['objects']))
    return bytes(h)+b''.join(w_obj(o,ver) for o in L['objects'])

def write(F, thumbs):
    ver=F['version']
    out=bytes.fromhex(F['hdr_hex'])+bytes.fromhex(F['hdr2_hex'])
    if ver>=0x54: out+=w_layer(F['global_layer'],ver)
    out+=struct.pack('<i',len(F['pages']))
    for i,P in enumerate(F['pages']):
        out+=bytes.fromhex(P['hdr_hex'])+struct.pack('<i',len(P['layers']))
        out+=b''.join(w_layer(L,ver) for L in P['layers'])
        out+=bytes.fromhex(P.get('cues_hex',''))
        if P['has_thumb']: out+=thumbs[i]
    return out

if __name__=='__main__':
    fn=sys.argv[1]; d=open(fn,'rb').read(); th=[]
    F=H.parse(d,th); out=write(F,dict(th))
    if len(sys.argv)>2: open(sys.argv[2],'wb').write(out)
    print(fn,'roundtrip', 'IDENTICAL' if out==d else 'DIFF (%d vs %d)'%(len(out),len(d)))
