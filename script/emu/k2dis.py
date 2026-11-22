# k2dis.py - pembaca libgame.so arm64 (pyelftools + capstone): disassembly beranotasi, string UTF-16, relokasi GOT.
# Pakai: K2_LIBGAME=/jalur/libgame.so python3 k2dis.py 0x5e16e8 596
# Pip: pip install capstone pyelftools. Divalidasi terhadap objdump: 1.829 instruksi BTOA identik (selisih hanya alias mov/orr).
import sys, struct
from elftools.elf.elffile import ELFFile
from capstone import Cs, CS_ARCH_ARM64, CS_MODE_ARM
import os
LIB=os.environ.get('K2_LIBGAME','Kirikiroid2_1.3.9/lib/arm64-v8a/libgame.so')   # lib/arm64-v8a/libgame.so dari APK 1.3.9
_f=open(LIB,'rb'); _e=ELFFile(_f)
SEGS=[(s['p_vaddr'],s['p_filesz'],s['p_offset']) for s in _e.iter_segments() if s['p_type']=='PT_LOAD']
def rd(va,n):
    for v,fs,off in SEGS:
        if v<=va<v+fs:
            _f.seek(off+va-v); return _f.read(min(n,v+fs-va))
    raise ValueError('va tidak terpetakan %x'%va)
def u32(va): return struct.unpack('<I',rd(va,4))[0]
def u64(va): return struct.unpack('<Q',rd(va,8))[0]
def cstr_u16(va,maxn=200):
    b=rd(va,maxn*2); s=''
    for i in range(0,len(b)-1,2):
        c=struct.unpack('<H',b[i:i+2])[0]
        if c==0: break
        s+=chr(c)
    return s
def cstr(va,maxn=200):
    b=rd(va,maxn); return b.split(b'\0')[0].decode('latin1')
_cs=Cs(CS_ARCH_ARM64,CS_MODE_ARM); _cs.detail=False
def dis(va,size):
    out=[]
    for i in _cs.disasm(rd(va,size),va):
        out.append((i.address,i.mnemonic,i.op_str))
    return out
if __name__=='__main__':
    va=int(sys.argv[1],16); n=int(sys.argv[2])
    for a,m,o in dis(va,n): print('  %x: %-6s %s'%(a,m,o))

# ---- simbol & relokasi ----
import subprocess, functools
@functools.lru_cache(None)
def _syms():
    d={}
    for secname in ('.dynsym','.symtab'):
        sec=_e.get_section_by_name(secname)
        if not sec: continue
        for s in sec.iter_symbols():
            if s['st_value'] and s['st_info']['type'] in ('STT_FUNC','STT_OBJECT','STT_TLS','STT_NOTYPE'):
                d.setdefault(s['st_value'],[]).append(s.name)
    return d
def demangle(names):
    if not names: return []
    p=subprocess.run(['c++filt'],input='\n'.join(names),capture_output=True,text=True)
    return p.stdout.strip().split('\n')
def symat(va):
    n=_syms().get(va)
    return demangle(n) if n else None
@functools.lru_cache(None)
def _relocs():
    d={}
    for secname in ('.rela.dyn','.rela.plt'):
        sec=_e.get_section_by_name(secname)
        if not sec: continue
        dyn=_e.get_section_by_name('.dynsym')
        for r in sec.iter_relocations():
            off=r['r_offset']; sym=r['r_info_sym']; add=r['r_addend']; t=r['r_info_type']
            nm=None
            if sym: nm=dyn.get_symbol(sym).name
            d[off]=(t,nm,add)
    return d
def got(va):
    r=_relocs().get(va)
    if not r: return None
    t,nm,add=r
    if nm: return demangle([nm])[0]+(('+%d'%add) if add else '')
    return 'REL->0x%x'%add

import re as _re
def annotate(ins, known=None):
    """ins: list (addr,mn,ops). Kembalikan baris teks beranotasi (adrp+add -> alamat, ldr GOT -> reloc, string)."""
    known=known or {}
    tg=set()
    for a,m,o in ins:
        if m in('b','bl') or m.startswith('b.') or m in('cbz','cbnz','tbz','tbnz'):
            t=o.split(',')[-1].strip().lstrip('#')
            if t.startswith('0x'): tg.add(int(t,16))
    pg={}; res=[]
    for a,m,o in ins:
        if a in tg: res.append('L%x:'%a)
        note=''
        if m=='adrp':
            r,v=[x.strip() for x in o.split(',')]; pg[r]=int(v.lstrip('#'),16)
        elif m=='add' and _re.match(r'x\d+, x\d+, #0x[0-9a-f]+$',o):
            rd,rn,im=[x.strip() for x in o.split(',')]
            if rn in pg:
                ad=pg[rn]+int(im.lstrip('#'),16); pg[rd]=ad; note='= 0x%x'%ad
                try:
                    s=cstr_u16(ad,60)
                    if len(s)>=2 and all(32<=ord(c)<127 for c in s): note+='  u16:"%s"'%s
                    else:
                        s=cstr(ad,60)
                        if len(s)>=3 and all(32<=ord(c)<127 for c in s): note+='  ascii:"%s"'%s
                except Exception: pass
        elif m=='ldr' and _re.match(r'[xw]\d+, \[x\d+, #0x[0-9a-f]+\]$',o):
            rt,rest=o.split(', ',1); rn=rest[1:].split(',')[0]; im=int(rest.split('#')[1].rstrip(']'),16)
            if rn in pg:
                ad=pg[rn]+im
                g=got(ad)
                note='[0x%x] '%ad+(('GOT '+g) if g else '')
        if m in('b','bl'):
            t=int(o.strip().lstrip('#'),16); note=known.get(t,'')
        if m.startswith('b.') or m in('cbz','cbnz','tbz','tbnz'):
            t=o.split(',')[-1].strip().lstrip('#')
            if t.startswith('0x') and int(t,16) in tg: note='-> L%x'%int(t,16)
        res.append('  %x: %-6s %-34s %s'%(a,m,o,note))
    return res
def show(va,size,known=None):
    print('\n'.join(annotate(dis(va,size),known)))
