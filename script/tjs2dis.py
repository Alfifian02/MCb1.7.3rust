#!/usr/bin/env python3
"""Disassembler bytecode TJS2 (format TJS2100) untuk analisis; mengikuti tjsByteCodeLoader.cpp."""
import struct, sys, re
names="NOP CONST CP CL CCL TT TF CEQ CDEQ CLT CGT SETF SETNF LNOT NF JF JNF JMP".split()
for g in "INC DEC LOR LAND BOR BXOR BAND SAR SAL SR ADD SUB MOD DIV IDIV MUL".split():
    names += [g, g+"PD", g+"PI", g+"P"]
names += "BNOT TYPEOF TYPEOFD TYPEOFI EVAL EEXP CHKINS ASC CHR NUM CHS INV CHKINV INT REAL STR OCTET CALL CALLD CALLI NEW GPD SPD SPDE SPDEH GPI SPI SPIE GPDS SPDS GPIS SPIS SETP GETP DELD DELI SRV RET ENTRY EXTRY THROW CHGTHIS GLOBAL ADDCI REGMEMBER DEBUGGER".split()
OP={n:i for i,n in enumerate(names)}
def load(path):
    b=open(path,'rb').read(); assert b[:4]==b'TJS2' and b[4:8]==b'100\0'
    u4=lambda o:struct.unpack_from('<i',b,o)[0]
    dsz=u4(16); o=20
    def arr(sz,fmt,pad=0):
        nonlocal o
        n=u4(o); o+=4; r=[]
        if sz==1: r=list(struct.unpack_from('<%db'%n,b,o)) if n else []; o+=(n+3)&~3
        elif sz==2: r=list(struct.unpack_from('<%dh'%n,b,o)) if n else []; o+=n*2+((n&1)*2)
        else: r=list(struct.unpack_from('<%d%s'%(n,fmt),b,o)) if n else []; o+=n*sz
        return r
    ARR={6:arr(1,'b'),7:arr(2,'h'),8:arr(4,'i'),9:arr(8,'q'),5:arr(8,'d')}  # tipe konstanta -> tabel nilai (id tipe mengikuti tjsByteCodeLoader; ASUMSI)
    n=u4(o); o+=4; S=[]
    for _ in range(n):
        l=u4(o); o+=4; S.append(b[o:o+l*2].decode('utf-16le','replace')); o+=l*2+((l&1)*2)
    n=u4(o); o+=4
    for _ in range(n):
        l=u4(o); o+=4; o+=((l+3)>>2)<<2
    o=12+dsz
    assert b[o:o+4]==b'OBJS'; o+=8
    top=u4(o); cnt=u4(o+4); o+=8; objs=[]
    for _ in range(cnt):
        assert b[o:o+4]==b'TJS2'; o+=8
        d={}
        d['parent'],name,d['ctype'],d['maxvar'],_,_,d['nargs'],_,_,d['setter'],d['getter'],d['super']=struct.unpack_from('<12i',b,o); o+=48
        d['name']=S[name]
        c=u4(o); o+=4; o+=c*8
        c=u4(o); o+=4; d['code']=list(struct.unpack_from('<%dh'%c,b,o)); o+=c*2+((c&1)*2)
        c=u4(o); o+=4; raw=struct.unpack_from('<%dh'%(c*2),b,o); o+=c*4
        data=[]
        for i in range(c):
            t,ix=raw[2*i],raw[2*i+1]
            data.append(('str',S[ix]) if t==3 else ('obj',ix) if t in(2,10) else ('void',None) if t==0 else ('num',ARR[t][ix]) if t in ARR and 0<=ix<len(ARR[t]) else ('t%d'%t,ix))
        d['data']=data
        c=u4(o); o+=4; o+=c*4
        c=u4(o); o+=4; d['props']=list(struct.unpack_from('<%di'%(c*2),b,o)); o+=c*8
        objs.append(d)
    return S,objs
def dis(d,objs):
    code=d['code']; data=d['data']; out=[]; i=0
    def K(ix):
        v=data[ix] if 0<=ix<len(data) else None
        if v is None: return '?'
        if v[0]=='str': return repr(v[1])
        if v[0]=='obj': return '<obj %d:%s>'%(v[1],objs[v[1]]['name'])
        if v[0]=='num': return '#%r'%(v[1],)
        return '%s:%s'%v
    while i<len(code):
        op=code[i]; n=names[op] if 0<=op<len(names) else '?%d'%op
        a=code[i+1:i+5]
        if n=='NOP' or n in('NF','EXTRY','RET','REGMEMBER','DEBUGGER'): sz=1;s=n.lower()
        elif n=='CONST': sz=3;s='const %%%d, *%d  ; %s'%(a[0],a[1],K(a[1]))
        elif n in('CP','CEQ','CDEQ','CLT','CGT','CHKINS','CHGTHIS','ADDCI','CCL') or (n[-1:]!='P' and n[:3] in('LOR','LAN','BOR','BXO','BAN','SAR','SAL','SR','ADD','SUB','MOD','DIV','IDI','MUL') and not n.endswith(('PD','PI'))):
            sz=3;s='%s %%%d, %%%d'%(n.lower(),a[0],a[1])
        elif n in('JF','JNF','JMP'): sz=2;s='%s %d'%(n.lower(),i+a[0])
        elif n=='ENTRY': sz=3;s='entry %d, %%%d'%(i+a[0],a[1])
        elif n in('GPD','GPDS','DELD','TYPEOFD'): sz=4;s='%s %%%d, %%%d.*%d ; %s'%(n.lower(),a[0],a[1],a[2],K(a[2]))
        elif n in('SPD','SPDE','SPDEH','SPDS'): sz=4;s='%s %%%d.*%d, %%%d ; %s'%(n.lower(),a[0],a[1],a[2],K(a[1]))
        elif n in('GPI','GPIS','DELI','TYPEOFI'): sz=4;s='%s %%%d, %%%d.%%%d'%(n.lower(),a[0],a[1],a[2])
        elif n in('SPI','SPIE','SPIS'): sz=4;s='%s %%%d.%%%d, %%%d'%(n.lower(),a[0],a[1],a[2])
        elif n in('SETP','GETP'): sz=3;s='%s %%%d, %%%d'%(n.lower(),a[0],a[1])
        elif n in('CALL','CALLD','CALLI','NEW'):
            st=5 if n in('CALLD','CALLI') else 4
            num=code[i+st-1]
            if n=='CALLD': h='calld %%%d, %%%d.*%d ; %s'%(a[0],a[1],a[2],K(a[2]))
            elif n=='CALLI': h='calli %%%d, %%%d.%%%d'%(a[0],a[1],a[2])
            else: h='%s %%%d, %%%d'%(n.lower(),a[0],a[1])
            if num==-1: sz=st; args='...'
            elif num==-2:
                st+=1; num=code[i+st-1]; sz=st+num*2; args=[]
                for j in range(num):
                    t,r=code[i+st+j*2],code[i+st+j*2+1]
                    args.append(('%%%d'%r) if t==0 else ('*%%%d'%r) if t==1 else ('%%%d*'%r))
                args=', '.join(args)
            else:
                sz=st+num; args=', '.join('%%%d'%x for x in code[i+st:i+st+num])
            s=h+' ('+args+')'
        elif n in('SRV','THROW','GLOBAL','TT','TF','SETF','SETNF','LNOT','BNOT','ASC','CHR','NUM','CHS','CL','INV','CHKINV','TYPEOF','EVAL','EEXP','INT','REAL','STR','OCTET','INC','DEC'): sz=2;s='%s %%%d'%(n.lower(),a[0])
        elif n in('INCPD','DECPD'): sz=4;s='%s %%%d, %%%d.*%d'%(n.lower(),a[0],a[1],a[2])
        elif n in('INCPI','DECPI'): sz=4;s='%s %%%d, %%%d.%%%d'%(n.lower(),a[0],a[1],a[2])
        elif n in('INCP','DECP'): sz=3;s='%s %%%d, %%%d'%(n.lower(),a[0],a[1])
        elif n.endswith('PD'): sz=5;s='%s %%%d, %%%d.*%d, %%%d ; %s'%(n.lower(),a[0],a[1],a[2],a[3],K(a[2]))
        elif n.endswith('PI'): sz=5;s='%s %%%d, %%%d.%%%d, %%%d'%(n.lower(),a[0],a[1],a[2],a[3])
        elif n.endswith('P'): sz=4;s='%s %%%d, %%%d, %%%d'%(n.lower(),a[0],a[1],a[2])
        else: sz=1;s='?%d'%op
        out.append('%05d %s'%(i,s)); i+=sz
    return out
if __name__=='__main__':
    S,objs=load(sys.argv[1]); pat=sys.argv[2] if len(sys.argv)>2 else None
    for k,d in enumerate(objs):
        L=dis(d,objs)
        if pat and not any(re.search(pat,x) for x in L) and not re.search(pat,d['name']): continue
        print('=== obj %d name=%r parent=%d ctype=%d nargs=%d'%(k,d['name'],d['parent'],d['ctype'],d['nargs']))
        print('\n'.join(L))
