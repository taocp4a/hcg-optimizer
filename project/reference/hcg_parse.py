#!/usr/bin/env python3
"""ez-CGER .hcg parser (format version 0x78 / 2.10.65). See project/docs/HCG_FORMAT.md."""
import struct, sys, json, zlib, os

class R:
    def __init__(s, d): s.d=d; s.o=0; s.cov=bytearray(len(d))
    def read(s, n, tag=''):
        b=s.d[s.o:s.o+n]
        if len(b)!=n: raise EOFError('short read %s at %#x'%(tag,s.o))
        for i in range(s.o,s.o+n): s.cov[i]=1
        s.o+=n; return b
    def u32(s): return struct.unpack('<I',s.read(4))[0]
    def i32(s): return struct.unpack('<i',s.read(4))[0]

def f32s(b): return list(struct.unpack('<%df'%(len(b)//4),b))
def i32s(b): return list(struct.unpack('<%di'%(len(b)//4),b))

def image_id(path, stamp):
    """subfolder file name prefix: CRC32(seed 'HDCG') over UTF-16LE(IntToHex(stamp,4)+lower(path))"""
    low=''.join(c.lower() if 'A'<=c<='Z' else c for c in path)  # ASCII-only lowercase (§6.3)
    s=('%04X'%stamp)+low
    return '%08X'%zlib.crc32(s.encode('utf-16le'),0x47434448)

def stamp_to_dt(q):
    import datetime
    days=q>>32; ms=q&0xffffffff
    return (datetime.datetime(1,1,1)+datetime.timedelta(days=days-1,milliseconds=ms)).isoformat()

def safebmp(r, ver):
    o=r.o; h=r.read(32,'SafeBMP hdr')
    magic,size,flag,stamp,res=struct.unpack('<8sqIQI',h)
    assert magic==b'SafeBMP ', 'bad SafeBMP at %#x'%o
    out={'offset':o,'size':size,'is_path':flag!=0,'stamp':stamp,'reserved':res}
    if size<=0: return out
    payload=r.read(size,'SafeBMP payload')
    if flag:
        p=payload.decode('utf-16le'); out['path']=p
        out['stamp_local']=stamp_to_dt(stamp); out['subfolder_name']=image_id(p,stamp)+os.path.basename(p.replace('\\','/'))
    else:
        out['embedded_bytes']=size; out['embedded_head']=payload[:16].hex(); out['payload']=payload
    return out

def wstr(r):
    n=r.i32(); return r.read(2*n).decode('utf-16le') if n>0 else ''

def richtext(r, ver):
    # §6.4 (ver>=100): WideString text, then one char-format record per character
    if ver<0x64: return None
    o=r.o; txt=wstr(r); chars=[]
    for ch in txt:
        b=r.read(11); i=struct.unpack('<4i',r.read(16)); b2=r.read(1)[0]; font=wstr(r)
        raw=(b+struct.pack('<4i',*i)+bytes([b2])).hex()
        chars.append({'raw':raw,'b0_style':b[0],'b1_flag9':b[1],'b2':b[2],'b3':b[3],'b4_flagC':b[4],'b5':b[5],'b6':b[6],
                      'color_rgba':b[7:11].hex(),'i14':i[0],'i18':i[1],'i1c':i[2],'i20':i[3],'b24':b2,'font':font})
    # compress runs
    runs=[]
    for c in chars:
        if runs and runs[-1]['fmt']==c: runs[-1]['n']+=1
        else: runs.append({'fmt':c,'n':1})
    return {'offset':o,'text':txt,'runs':runs}

def obj(r, ver, depth=0):
    o=r.o; rec=r.read(0xf8,'obj record')
    O={'offset':o,'type':struct.unpack_from('<i',rec,0x1c)[0],
       'center':f32s(rec[0:8]),'f08':f32s(rec[8:12]),'rect':f32s(rec[0xc:0x1c]),
       'rec_hex':rec.hex()}
    n_arr=struct.unpack_from('<i',rec,0xf0)[0]
    if ver>=0x76 and n_arr>0: O['arr']=i32s(r.read(4*n_arr))
    sh=r.read(0x20,'obj subhdr')
    has_img,has_ext=sh[0],sh[1]
    n1,len1,len2,len3,nimg,len4=struct.unpack_from('<6i',sh,4)
    O['sub']={'has_image':has_img,'has_ext':has_ext,'n_anim':n1,'len_text':len1,'len_text2':len2,'len_path':len3,'n_images':nimg,'len4':len4,'raw':sh.hex()}
    if has_img: O['image']=safebmp(r,ver)
    for i in range(max(nimg,0)): O.setdefault('images',[]).append(safebmp(r,ver))
    O['tail16']=i32s(r.read(8)) + (i32s(r.read(8)) if ver>=0x15 else [])
    if ver<0x29 and len4>0: raise NotImplementedError('old len4')
    if n1>0:  # §6.1c point list: n x {i32 kind, i32 x, i32 y} for ver>=0xF (polyline/figure vertices)
        if ver<0xf: raise NotImplementedError('old point list')
        O['points']=[list(struct.unpack('<3i',r.read(12))) for _ in range(n1)]
    if len1>0: O['text']=r.read(2*len1).decode('utf-16le')
    if len2>0: O['text2']=r.read(2*len2).decode('utf-16le')
    if len3>0:
        O['path']=r.read(2*len3).decode('utf-16le')
        if ver>=0x6e: O['path_stamp']=struct.unpack('<Q',r.read(8))[0]
    if has_ext: O['rich']=richtext(r,ver)
    if ver<0x29 and len4>0: r.read(2*len4)
    return O

def layer(r, ver):
    o=r.o; h=r.read(0x18 if ver>=0x28 else 4,'layer hdr')
    n=struct.unpack_from('<i',h,0)[0]
    L={'offset':o,'n_objects':n,'hdr':i32s(h),'hdr_hex':h.hex(),'objects':[]}
    for i in range(n): L['objects'].append(obj(r,ver))
    return L

def bmp(r):
    o=r.o; h=r.d[o:o+14]
    assert h[:2]==b'BM', 'no BM at %#x'%o
    size=struct.unpack_from('<I',h,2)[0]
    b=r.read(size); w,hh,_,bpp=struct.unpack_from('<iiHH',b,18)
    return {'offset':o,'size':size,'w':w,'h':hh,'bpp':bpp},b

def page(r, ver, idx, thumbs):
    o=r.o; h=r.read(0x6c,'page hdr')
    P={'offset':o,'hdr':i32s(h),'hdr_hex':h.hex(),'has_thumb':h[2],'layers':[]}
    nl=r.i32(); P['n_layers']=nl
    for i in range(nl): P['layers'].append(layer(r,ver))
    n40=struct.unpack_from('<i',h,8)[0]
    if n40>0: P['cues_hex']=r.read(40*n40).hex()
    if h[2]:
        P['thumb'],b=bmp(r); thumbs.append((idx,b))
    return P

def parse(d, thumbs=None):
    thumbs=[] if thumbs is None else thumbs
    r=R(d); h=r.read(0x28,'file hdr')
    assert h[:8]==b'HDCGData'
    ver=struct.unpack_from('<i',h,8)[0]
    F={'magic':'HDCGData','version':ver,'hdr':i32s(h[8:]),'hdr_hex':h.hex(),'hdr2_hex':r.read(0x20).hex()}
    F['hdr2']=i32s(bytes.fromhex(F['hdr2_hex']))
    if ver>=0x54: F['global_layer']=layer(r,ver)
    n=r.i32(); F['n_pages']=n; F['pages']=[page(r,ver,i,thumbs) for i in range(n)]
    F['end']=r.o; F['file_size']=len(d); F['trailing']=len(d)-r.o
    F['covered_pct']=100*sum(r.cov)/len(d)
    return F

if __name__=='__main__':
    fn=sys.argv[1]; d=open(fn,'rb').read(); th=[]
    F=parse(d,th)
    outdir=os.path.splitext(os.path.basename(fn))[0]+'_out'; os.makedirs(outdir,exist_ok=True)
    for i,b in th: open(f'{outdir}/thumb_{i+1}.bmp','wb').write(b)
    json.dump(F,open(f'{outdir}/parsed.json','w'),ensure_ascii=False,indent=1,default=lambda b:'<%d bytes>'%len(b))
    print(fn,'ver',hex(F['version']),'pages',F['n_pages'],'end',hex(F['end']),'size',hex(len(d)),'covered %.3f%%'%F['covered_pct'])
