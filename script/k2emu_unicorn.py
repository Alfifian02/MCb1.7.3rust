# k2emu_unicorn.py - menjalankan FUNGSI libgame.so arm64 di emulator (unicorn) untuk uji diferensial di PC/sandbox.
# Pasang:  pip install unicorn          Pakai: LIBGAME=/jalur/libgame.so python3 ...
# Cara: peta seluruh segmen PT_LOAD (base 0), siapkan stack/TLS, tiru callee dengan Emu.stubs[alamat] = f(emu)
# (stub dijalankan saat instruksi di alamat itu akan dieksekusi, lalu kembali ke x30). Contoh: docs/spec/ dan
# tests/plugins/layerExImage/gen_golden.py. Hanya untuk fungsi tanpa efek samping sistem; BUKAN pengganti uji perangkat.
# emu.py - menjalankan fungsi libgame.so arm64 di unicorn (hanya untuk pengujian di sandbox)
import struct
from unicorn import *
from unicorn.arm64_const import *
import os
LIBP=os.environ.get('LIBGAME', os.path.expanduser('~/k2_out/libgame.so'))
D=open(LIBP,'rb').read()
def segs():
    phoff,=struct.unpack_from('<Q',D,0x20); es,n=struct.unpack_from('<HH',D,0x36); r=[]
    for i in range(n):
        t,fl,off,va,pa,fsz,msz,al=struct.unpack_from('<IIQQQQQQ',D,phoff+i*es)
        if t==1: r.append((va,off,fsz,msz))
    return r
PG=0x1000
class Emu:
    def __init__(s):
        s.uc=Uc(UC_ARCH_ARM64,UC_MODE_ARM)
        for va,off,fsz,msz in segs():
            a=va&~(PG-1); e=(va+msz+PG-1)&~(PG-1)
            s.uc.mem_map(a,e-a); s.uc.mem_write(va,D[off:off+fsz])
        s.uc.mem_map(0x70000000,0x400000)   # stack (grows down from top)
        s.uc.mem_map(0x71000000,0x10000)    # TLS
        s.uc.mem_map(0x72000000,0x4000000)  # heap/data arena
        s.uc.reg_write(UC_ARM64_REG_CPACR_EL1,0x300000)
        s.uc.reg_write(UC_ARM64_REG_TPIDR_EL0,0x71000000)
        s.stubs={}; s.heap=0x72000000; s.calls=[]
        s.uc.hook_add(UC_HOOK_CODE,s._hook)
    def _hook(s,uc,addr,size,ud):
        f=s.stubs.get(addr)
        if f is not None:
            s.calls.append(addr)
            r=f(s)
            if r is not None: uc.reg_write(UC_ARM64_REG_X0,r&0xFFFFFFFFFFFFFFFF)
            uc.reg_write(UC_ARM64_REG_PC,uc.reg_read(UC_ARM64_REG_X30))
    def alloc(s,n,al=16):
        s.heap=(s.heap+al-1)&~(al-1); p=s.heap; s.heap+=n; return p
    def w(s,a,b): s.uc.mem_write(a,bytes(b))
    def r(s,a,n): return bytes(s.uc.mem_read(a,n))
    def w32(s,a,v): s.uc.mem_write(a,struct.pack('<I',v&0xFFFFFFFF))
    def w64(s,a,v): s.uc.mem_write(a,struct.pack('<Q',v&0xFFFFFFFFFFFFFFFF))
    def R(s,n): return s.uc.reg_read(n)
    def call(s,addr,x=(),d=(),maxn=200_000_000):
        uc=s.uc; sp=0x70000000+0x400000-0x100
        uc.reg_write(UC_ARM64_REG_SP,sp)
        for i,v in enumerate(x): uc.reg_write(UC_ARM64_REG_X0+i,v&0xFFFFFFFFFFFFFFFF)
        for i,v in enumerate(d): uc.reg_write(UC_ARM64_REG_D0+i,struct.unpack('<Q',struct.pack('<d',v))[0])
        RET=0x7FFF0000
        try: uc.mem_map(RET,PG)
        except Exception: pass
        uc.reg_write(UC_ARM64_REG_X30,RET)
        uc.emu_start(addr,RET,count=maxn)
        return uc.reg_read(UC_ARM64_REG_X0)
