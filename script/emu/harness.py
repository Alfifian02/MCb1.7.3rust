# harness.py - emulasi AArch64 (Unicorn) fungsi ASLI libgame.so 1.3.9 pada data uji. Objek Layer/Bitmap/Texture/TJS
# adalah tiruan minimal (hanya yang dibaca fungsi yang dibongkar). Dipakai btoa_golden.py untuk membuat vektor emas.
# Pip: pip install unicorn capstone pyelftools ; env K2_LIBGAME=/jalur/libgame.so
# Harness emulasi AArch64 (Unicorn) untuk menjalankan fungsi ASLI libgame.so 1.3.9 pada data uji.
# Objek Layer/Bitmap/Texture/TJS adalah tiruan minimal; hanya yang dibaca fungsi yang dibongkar.
import os, sys, struct
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
import k2dis as D
from unicorn import *
from unicorn.arm64_const import *

R=UC_ARM64_REG_X0
def X(n): return getattr(__import__('unicorn.arm64_const',fromlist=['x']), 'UC_ARM64_REG_X%d'%n)
PAGE=0x1000
def al(v,a=PAGE): return (v+a-1)//a*a

class Harness:
    STUBS=0x7d000000; STACK=0x7f000000; TLS=0x7e000000; HEAP=0x60000000; SENT=0x7c000000
    def __init__(self):
        uc=Uc(UC_ARCH_ARM64,UC_MODE_ARM); self.uc=uc
        segs=[(s['p_vaddr'],s['p_filesz'],s['p_offset'],s['p_memsz']) for s in D._e.iter_segments() if s['p_type']=='PT_LOAD']
        end=al(max(v+m for v,f,o,m in segs))
        uc.mem_map(0,end,UC_PROT_ALL)
        for v,f,o,m in segs:
            D._f.seek(o); uc.mem_write(v,D._f.read(f))
        # relokasi RELATIVE
        for sec in ('.rela.dyn',):
            s=D._e.get_section_by_name(sec)
            for r in s.iter_relocations():
                if r['r_info_type']==1027: uc.mem_write(r['r_offset'],struct.pack('<Q',r['r_addend']))
        uc.mem_map(self.STACK,0x100000,UC_PROT_ALL)
        uc.mem_map(self.TLS,PAGE,UC_PROT_ALL); uc.mem_write(self.TLS+0x28,struct.pack('<Q',0x1122334455667788))
        uc.reg_write(UC_ARM64_REG_TPIDR_EL0,self.TLS)
        uc.mem_map(self.HEAP,64<<20,UC_PROT_ALL); self.brk=self.HEAP
        uc.mem_map(self.STUBS,0x10000,UC_PROT_ALL); uc.mem_map(self.SENT,PAGE,UC_PROT_ALL)
        uc.reg_write(UC_ARM64_REG_CPACR_EL1,0x300000)   # aktifkan FP/NEON
        self.nstub=0; self.hooks={}; self.reg={}   # reg: ptr -> dict objek tiruan
        self.software=True; self.log=[]; self.updates=[]; self.calls=[]; self.thrown=None; self.stackfail=False
        self._install()
        self.base_brk=self.brk
    def reset_case(self):
        self.brk=self.base_brk; self.reg.clear(); self.px=[]; self.oob=False
    GUARD=0x800
    def alloc_px(self,n):
        # buffer piksel: celah 0x2000 di kiri/kanan; akses dalam GUARD byte di luar buffer ditandai self.oob
        self.brk+=0x2000; p=self.brk; self.brk+=al(n,16)+0x2000
        self.uc.mem_write(p,b'\0'*n); self.px.append((p,p+n)); return p
    def _mem_cb(self,uc,access,addr,size,value,ud):
        for lo,hi in self.px:
            if lo-self.GUARD<=addr<lo or hi<=addr<hi+self.GUARD: self.oob=True; return
    # --- alokasi ---
    def alloc(self,n,fill=0):
        p=self.brk; self.brk+=al(n+16,16)
        self.uc.mem_write(p,bytes([fill])*n); return p
    def w(self,addr,data): self.uc.mem_write(addr,data)
    def r(self,addr,n): return bytes(self.uc.mem_read(addr,n))
    def u32(self,a): return struct.unpack('<I',self.r(a,4))[0]
    def i32(self,a): return struct.unpack('<i',self.r(a,4))[0]
    def u64(self,a): return struct.unpack('<Q',self.r(a,8))[0]
    def p64(self,a,v): self.w(a,struct.pack('<Q',v))
    def p32(self,a,v): self.w(a,struct.pack('<I',v&0xffffffff))
    def reg_(self,n): return self.uc.reg_read(X(n))
    def setreg(self,n,v): self.uc.reg_write(X(n),v&0xffffffffffffffff)
    # --- hook ---
    def hook(self,addr,fn):
        self.hooks[addr]=fn
    def stub(self,fn):
        a=self.STUBS+self.nstub*16; self.nstub+=1; self.uc.mem_write(a,b'\xc0\x03\x5f\xd6'); self.hooks[a]=fn; return a
    def _code_cb(self,uc,addr,size,ud):
        fn=self.hooks.get(addr)
        if fn is None: return
        lr=uc.reg_read(UC_ARM64_REG_X30)
        stop=fn(self)
        if stop=='STOP': uc.emu_stop(); return
        uc.reg_write(UC_ARM64_REG_PC,lr)
    def _install(self):
        uc=self.uc
        H=self
        def throw_msg(h):
            p=h.reg_(0)
            try: s=D.cstr_u16(0,0) if False else ''.join(chr(struct.unpack('<H',h.r(p+2*i,2))[0]) for i in range(80) if struct.unpack('<H',h.r(p+2*i,2))[0]!=0 and all(struct.unpack('<H',h.r(p+2*j,2))[0]!=0 for j in range(i+1)))
            except Exception: s='?'
            h.thrown=('msg',s); return 'STOP'
        def throw_param(h): h.thrown=('param',h.reg_(1)&0xffffffff); return 'STOP'
        def stackfail(h): h.stackfail=True; return 'STOP'
        def newarr(h): h.setreg(0,h.alloc(h.reg_(0))); 
        def noop(h): pass
        def memset(h):
            d,c,n=h.reg_(0),h.reg_(1)&0xff,h.reg_(2); h.w(d,bytes([c])*n)
        def issoft(h): h.setreg(0,1 if h.software else 0)
        def update(h):
            rc=struct.unpack('<4i',h.r(h.reg_(1),16)); h.updates.append(rc)
        def getlayerclass(h): h.setreg(0,0)
        for a,f in ((0x953d0c,throw_msg),(0xa0cd8c,throw_param),(0x407130,stackfail),(0x41f9e0,newarr),(0x426f20,noop),
                    (0x40cdc0,memset),(0x848bdc,issoft),(0x807818,update),(0x5e14a4,getlayerclass),(0xa0e078,noop)):
            self.hooks[a]=f
        # vtable objek TJS (iTJSDispatch2) & tekstur
        def addref(h): h.setreg(0,1)
        def nis(h):
            obj=h.reg_(0); out=h.reg_(3)
            lay=h.reg.get(obj,{}).get('layer')
            if lay is None: h.setreg(0,0xfffffffffffffc19&0xffffffffffffffff); return   # gagal (negatif)
            h.p64(out,lay); h.setreg(0,0)
        def funccall(h):
            name=''.join(chr(struct.unpack('<H',h.r(h.reg_(2)+2*i,2))[0]) for i in range(32) if all(struct.unpack('<H',h.r(h.reg_(2)+2*j,2))[0]!=0 for j in range(i+1)))
            n=h.reg_(5)&0xffffffff; ps=h.reg_(6); vals=[]
            for i in range(n):
                v=h.u64(ps+8*i); ty=h.u32(v+0x10); vals.append((ty,struct.unpack('<q',h.r(v,8))[0]))
            h.calls.append((name,vals)); h.setreg(0,0)
        self.st_addref=self.stub(addref); self.st_nis=self.stub(nis); self.st_funccall=self.stub(funccall)
        self.vt_obj=self.alloc(0x100)
        self.p64(self.vt_obj+0x00,self.st_addref); self.p64(self.vt_obj+0x08,self.st_addref)
        self.p64(self.vt_obj+0x10,self.st_funccall); self.p64(self.vt_obj+0xc8,self.st_nis)
        def t_read(h):
            t=h.reg.get(h.reg_(0)); l=h.reg_(1)&0xffffffff; h.setreg(0,t['base']+l*t['pitch'])
        def t_write(h):
            t=h.reg.get(h.reg_(0)); l=h.reg_(1)&0xffffffff; h.setreg(0,t['base']+l*t['pitch'])
        def t_pitch(h): h.setreg(0,h.reg[h.reg_(0)]['pitch'])
        def t_indep(h): h.setreg(0,0)   # vt[0x70] != 0 => jalur copy-on-write; 0 => tulis langsung
        def m_gettex(h): h.setreg(0,h.reg[h.reg_(0)]['tex'])
        self.vt_tex=self.alloc(0x100)
        self.p64(self.vt_tex+0x38,self.stub(t_read)); self.p64(self.vt_tex+0x48,self.stub(t_write))
        self.p64(self.vt_tex+0x50,self.stub(t_pitch)); self.p64(self.vt_tex+0x70,self.stub(t_indep))
        self.vt_main=self.alloc(0x100); self.p64(self.vt_main+0x10,self.stub(m_gettex))
        uc.hook_add(UC_HOOK_CODE,self._code_cb)
        self.px=[]; self.oob=False
        uc.hook_add(UC_HOOK_MEM_READ|UC_HOOK_MEM_WRITE,self._mem_cb,None,self.HEAP,self.HEAP+(64<<20)-1)
    # --- pembuat objek tiruan ---
    def mk_tex(self,w,h,data,pitch=None):
        pitch=pitch or (len(data)//max(h,1))
        base=self.alloc_px(max(len(data),1)); self.w(base,data)
        t=self.alloc(0x40); self.p64(t,self.vt_tex); self.p32(t+8,1); self.p32(t+0xc,w); self.p32(t+0x10,h)
        self.reg[t]={'base':base,'pitch':pitch,'w':w,'h':h}; return t
    def mk_image(self,w,h,data,pitch=None):
        t=self.mk_tex(w,h,data,pitch)
        m=self.alloc(0x100); self.p64(m,self.vt_main); self.p64(m+0x58,t)
        self.reg[m]={'tex':t}; return m,t
    def mk_layer(self,w,h,pix=None,prov=None,clip=None,ppitch=None,bpp=4):
        L=self.alloc(0x400)
        if pix is not None:
            m,t=self.mk_image(w,h,pix); self.p64(L+0x118,m)
        if prov is not None:
            pm,pt=self.mk_image(w,h,prov,ppitch); self.p64(L+0x128,pm)
        clip=clip or (0,0,w,h); self.w(L+0x19c,struct.pack('<4i',*clip))
        obj=self.alloc(0x40); self.p64(obj,self.vt_obj); self.reg[obj]={'layer':L}
        return {'L':L,'obj':obj,'w':w,'h':h}
    def texdata(self,lay):
        m=self.u64(lay['L']+0x118); t=self.reg[m]['tex']; d=self.reg[t]; return self.r(d['base'],d['pitch']*d['h'])
    def provdata(self,lay):
        m=self.u64(lay['L']+0x128); t=self.reg[m]['tex']; d=self.reg[t]; return self.r(d['base'],d['pitch']*d['h'])
    def variant_int(self,v):
        p=self.alloc(0x20); self.w(p,struct.pack('<q',v)); self.p32(p+0x10,4); return p
    def variant_obj(self,obj):
        p=self.alloc(0x20); self.p64(p,obj); self.p32(p+0x10,1); return p
    def variant_void(self):
        p=self.alloc(0x20); self.p32(p+0x10,0); return p
    def call(self,func,lay,params,software=True,limit=50_000_000):
        self.software=software; self.updates=[]; self.calls=[]; self.thrown=None; self.stackfail=False
        n=len(params); arr=self.alloc(8*max(n,1))
        for i,p in enumerate(params): self.p64(arr+8*i,p)
        uc=self.uc
        sp=self.STACK+0xff000
        uc.reg_write(UC_ARM64_REG_SP,sp)
        self.setreg(0,0); self.setreg(1,n); self.setreg(2,arr); self.setreg(3,lay['obj'])
        uc.reg_write(UC_ARM64_REG_X30,self.SENT)
        try: uc.emu_start(func,self.SENT,count=limit)
        except UcError as e: self.thrown=('uc',str(e))
        ret=uc.reg_read(UC_ARM64_REG_X0)&0xffffffff
        if ret>=0x80000000: ret-=1<<32
        return ret
