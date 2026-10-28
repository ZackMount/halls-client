#!/usr/bin/env python3
"""Compile the procedural entity formulas to a tiny, non-recursive stack program.
No vertices, textures, samples, or precomputed animation are stored in the result.
"""
import ast
import re
import struct
import sys
from pathlib import Path
ROOT = Path(__file__).resolve().parent.parent
OPS = 'SET INDEX PUT ADD SUB MUL DIV NEG LT EQ JUMP ZERO CALL RETURN VEC SIN COS ABS SQRT POW REM MIN MAX DOT CROSS QUAD POP RQUAD'.split()
OP = {name: i for i, name in enumerate(OPS)}
assert {OP[name] for name in 'ADD SUB MUL DIV LT EQ MIN MAX DOT CROSS'.split()} == set(range(3,7)) | {8,9} | set(range(21,25))
labels = re.findall(r'\.short Lvm_(\w+)-Lvm_table', (ROOT/'src/entity_vm.s').read_text())
assert labels == [('native' if op in ('POW','REM','QUAD','RQUAD') else op.lower()) for op in OPS], 'Assembly dispatch table differs from compiler opcodes'
NATIVE = {name:name.upper() for name in 'vec sin cos abs sqrt pow rem min max dot cross quad'.split()}
NATIVE['rage_quad']='RQUAD'
source = ast.parse((ROOT / 'src/entity_model.hvm').read_text())
functions = {f.name: f for f in source.body}
returns = {f.name:any(isinstance(n,ast.Return) and n.value is not None for n in ast.walk(f)) for f in functions.values()}
code, constants, registers, entries = [], [], {}, []
# Registers 0..15 hold the rig; 16..18 hold streaming cable state.
calls={f.name:{n.func.id for n in ast.walk(f) if isinstance(n,ast.Call) and isinstance(n.func,ast.Name) and n.func.id in functions} for f in functions.values()}
def descendants(name):
    return calls[name] | set().union(*(descendants(child) for child in calls[name]))
reach={name:descendants(name) for name in functions}
for f in sorted(functions.values(),key=lambda f:(f.name!='draw', -len(list(ast.walk(f))))):
    names = [a.arg for a in f.args.args]
    for node in ast.walk(f):
        if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Store) and node.id not in names and node.id != 'unused':
            names.append(node.id)
    used=set(range(19))
    for other,slots in registers.items():
        if other in reach[f.name] or f.name in reach[other] or other=='draw' or f.name=='draw':used.update(slots.values())
    base=19
    while any(i in used for i in range(base,base+len(names))):base+=1
    registers[f.name]={name:base+i for i,name in enumerate(names)}
next_register=max(max(r.values()) for r in registers.values())+1
assert next_register <= 256

def emit(op, *args):
    if op == 'GET': code.append(48 + args[0])
    elif op == 'CONST': code.append(48 + next_register + args[0])
    else: code.extend([OP[op], *args])

def jump(op):
    emit(op, 0, 0)
    return len(code) - 2

def patch(at, target=None):
    target = len(code) if target is None else target
    code[at:at+2] = struct.pack('<H', target)

def constant(v):
    b = struct.pack('<f', float(v))
    if b not in constants:
        constants.append(b)
    assert len(constants) + next_register + 48 <= 256
    emit('CONST', constants.index(b))

loop_bounds = {}
def rig_bound(n):
    if isinstance(n,ast.Constant):return (n.value,n.value)
    if isinstance(n,ast.Name):return loop_bounds[n.id]
    a,b=rig_bound(n.left),rig_bound(n.right)
    if isinstance(n.op,ast.Add):return (a[0]+b[0],a[1]+b[1])
    assert isinstance(n.op,ast.Mult)
    products=[x*y for x in a for y in b]
    return (min(products),max(products))
def check_rig(n):
    lo,hi=rig_bound(n)
    assert 0 <= lo <= hi < 19, 'Rig indexing must be bounded at compile time'
def expression(n, regs):
    if isinstance(n, ast.Constant):
        constant(n.value)
    elif isinstance(n, ast.Name):
        if n.id == 'pi': constant(3.141592653589793)
        else: emit('GET', regs[n.id] if n.id in regs else registers['draw'][n.id])
    elif isinstance(n, ast.Subscript):
        assert isinstance(n.value, ast.Name) and n.value.id == 'rig'
        check_rig(n.slice)
        expression(n.slice, regs); emit('INDEX')
    elif isinstance(n, ast.UnaryOp):
        assert isinstance(n.op, ast.USub)
        expression(n.operand, regs); emit('NEG')
    elif isinstance(n, ast.BinOp):
        expression(n.left, regs); expression(n.right, regs)
        emit({ast.Add:'ADD',ast.Sub:'SUB',ast.Mult:'MUL',ast.Div:'DIV'}[type(n.op)])
    elif isinstance(n, ast.Compare):
        assert len(n.ops) == 1
        expression(n.left, regs); expression(n.comparators[0], regs)
        emit({ast.Lt:'LT',ast.Eq:'EQ'}[type(n.ops[0])])
    elif isinstance(n, ast.IfExp):
        expression(n.test, regs); no = jump('ZERO')
        expression(n.body, regs); done = jump('JUMP'); patch(no)
        expression(n.orelse, regs); patch(done)
    elif isinstance(n, ast.Call):
        assert isinstance(n.func, ast.Name)
        for arg in n.args: expression(arg, regs)
        if n.func.id in NATIVE: emit(NATIVE[n.func.id])
        else:
            f = functions[n.func.id]
            assert len(n.args) == len(f.args.args)
            emit('CALL', list(functions).index(n.func.id))
    else: raise ValueError(ast.dump(n))

def statements(nodes, regs):
    for n in nodes:
        if isinstance(n, ast.Assign):
            assert len(n.targets)==1
            t=n.targets[0]
            if isinstance(t, ast.Subscript):
                assert t.value.id == 'rig'
                check_rig(t.slice)
                expression(t.slice, regs); expression(n.value, regs); emit('PUT')
            else:
                assert t.id not in loop_bounds, 'Loop counters cannot be reassigned'
                expression(n.value, regs)
                if t.id == 'unused':
                    assert isinstance(n.value,ast.Call) and not returns[n.value.func.id]
                else:emit('SET',regs[t.id])
        elif isinstance(n, ast.Return):
            # All procedures return one vector (zero if no value is needed).
            expression(n.value or ast.Constant(0), regs); emit('RETURN')
        elif isinstance(n, ast.Expr):
            expression(n.value, regs)
            # Side-effect native operations return nothing; procedures return zero.
            assert isinstance(n.value,ast.Call)
            if n.value.func.id in functions:
                if returns[n.value.func.id]:emit('POP')
            else:assert n.value.func.id in ('quad','rage_quad')
        elif isinstance(n, ast.If):
            expression(n.test, regs); no=jump('ZERO'); statements(n.body,regs)
            if n.orelse:
                done=jump('JUMP');patch(no);statements(n.orelse,regs);patch(done)
            else:patch(no)
        elif isinstance(n, ast.For):
            assert n.iter.func.id=='range' and len(n.iter.args)==1
            constant(0);emit('SET',regs[n.target.id]);begin=len(code)
            emit('GET',regs[n.target.id]);expression(n.iter.args[0],regs);emit('LT');done=jump('ZERO')
            saved=loop_bounds.get(n.target.id)
            bound=n.iter.args[0]
            if isinstance(bound,ast.Constant):loop_bounds[n.target.id]=(0,bound.value-1)
            statements(n.body,regs)
            if saved is None:loop_bounds.pop(n.target.id,None)
            else:loop_bounds[n.target.id]=saved
            emit('GET',regs[n.target.id]);constant(1);emit('ADD');emit('SET',regs[n.target.id])
            back=jump('JUMP');patch(back,begin);patch(done)
        else:raise ValueError(ast.dump(n))

for f in functions.values():
    regs=registers[f.name]
    entries.append((len(code), next(iter(regs.values())), len(f.args.args)))
    statements(f.body,regs)
    if not isinstance(f.body[-1],ast.Return):emit('RETURN')
assert len(code)<65536
# Prove stack/control-flow/call bounds before emitting unchecked VM operands.
# Executable inputs are compiled here; the game never accepts external bytecode.
limits={}
def verify_function(fi, visiting=()):
    if fi in limits:return limits[fi]
    assert fi not in visiting, 'Recursive formula calls are forbidden'
    name=list(functions)[fi]
    entry,base,argc=entries[fi]
    assert argc>0 and base+argc<=next_register
    end=entries[fi+1][0] if fi+1<len(entries) else len(code)
    instructions={};pc=entry
    while pc<end:
        op=code[pc];size=1
        if op<48:
            assert op<len(OPS)
            if OPS[op] in ('SET','CALL'):size=2
            if OPS[op] in ('JUMP','ZERO'):size=3
        else:assert op<48+next_register+len(constants)
        instructions[pc]=(op,pc+size);pc+=size
    assert pc==end
    pending=[(entry,0)];depths={};peak=0;frames=1
    while pending:
        pc,depth=pending.pop()
        if pc in depths:
            assert depths[pc]==depth, 'Unequal branch stack heights'
            continue
        assert pc in instructions
        depths[pc]=depth
        op,nxt=instructions[pc]
        if op>=48:take,give=0,1
        else:
            op=OPS[op]
            if op=='RETURN':
                assert depth==int(returns[name]);continue
            if op=='CALL':
                child=code[pc+1];assert child<len(entries)
                take=entries[child][2];give=int(returns[list(functions)[child]])
                child_peak,child_frames=verify_function(child,(*visiting,fi))
                peak=max(peak,depth-take+child_peak);frames=max(frames,1+child_frames)
            elif op in ('SET','POP','ZERO'):take,give=1,0
            elif op=='PUT':take,give=2,0
            elif op in ('QUAD','RQUAD'):take,give=4,0
            elif op=='VEC':take,give=3,1
            elif op=='JUMP':take,give=0,0
            elif op in ('INDEX','NEG','SIN','COS','ABS','SQRT'):take,give=1,1
            else:take,give=2,1
            if op=='SET':assert code[pc+1]<next_register
        assert depth>=take, 'Formula stack underflow'
        depth+=give-take;peak=max(peak,depth)
        if op in ('JUMP','ZERO'):
            target=code[pc+1]+256*code[pc+2]
            assert entry<=target<end
            pending.append((target,depth))
            if op=='JUMP':continue
        pending.append((nxt,depth))
    assert len(depths)==len(instructions), 'Unreachable or overlapping code'
    assert peak<=32 and frames<=8, (name,peak,frames)
    limits[fi]=(peak,frames)
    return limits[fi]
for fi in range(len(entries)):verify_function(fi)

out='// Generated by tools/compile_entity.py from entity_model.hvm.\n'
out+='pub const REGISTERS: usize = '+str(next_register)+';\n'
for name,n in OP.items():out+=f'pub const {name}: u8 = {n};\n'
for name,i in zip(functions,range(len(functions))):out+=f'pub const FN_{name.upper()}: usize = {i};\n'
out+='#[repr(C)]\n#[derive(Clone,Copy)]\npub struct Function {pub entry:u16,pub base:u8,pub count:u8}\n'
out+='pub static FUNCTIONS: [Function; '+str(len(entries))+'] = ['+','.join(f'Function{{entry:{e},base:{b},count:{c}}}' for e,b,c in entries)+'];\n'
out+='pub static CONSTANTS: [f32; '+str(len(constants))+'] = ['+','.join(f'f32::from_bits(0x{struct.unpack("<I",b)[0]:08x})' for b in constants)+'];\n'
out+='pub static CODE: [u8; '+str(len(code))+'] = [\n'+''.join('    '+','.join(map(str,code[i:i+32]))+',\n' for i in range(0,len(code),32))+'];\n'
path=ROOT/'src/entity_program.rs'
if '--check' in sys.argv:assert path.read_text()==out,'Regenerate entity_program.rs'
else:path.write_text(out)
print(f'Entity program: {len(code)} code bytes, {len(constants)*4} constant bytes, {len(entries)*4} function bytes; {next_register} registers')

print('Verified stack/call maxima:', max(v[0] for v in limits.values()), max(v[1] for v in limits.values()))
