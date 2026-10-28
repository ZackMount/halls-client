// Win64 ABI; used unchanged through Rosetta for instruction-level verification.
.text
.globl {entry}
.seh_proc {entry}
{entry}:
    push rbx
.seh_pushreg rbx
    push rbp
.seh_pushreg rbp
    push rsi
.seh_pushreg rsi
    push rdi
.seh_pushreg rdi
    push r12
.seh_pushreg r12
    push r13
.seh_pushreg r13
    push r14
.seh_pushreg r14
    push r15
.seh_pushreg r15
    sub rsp, 664
.seh_stackalloc 664
.seh_endprologue
    mov rbx, rcx
    mov rsi, rdx
    lea rbp, [rsp+32]
    mov [rbp], r8
    lea rdi, [rsp+80]
    lea r15, [rsp+592]
    mov qword ptr [r15], 0
    add r15, 8
    lea r12, [rip+{code}]
    lea r13, [rip+{constants}]
    lea r14, [rip+{functions}]
Lvm_next:
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, {constant_base}
    jae Lvm_const
    cmp eax, 48
    jae Lvm_get
    lea ecx,[eax-3]
    cmp ecx,3
    jbe Lvm_binary
    lea ecx,[eax-8]
    cmp ecx,1
    jbe Lvm_binary
    lea ecx,[eax-21]
    cmp ecx,3
    ja Lvm_dispatch
Lvm_binary:
    sub rdi,32
    movaps xmm0,[rdi]
    movaps xmm1,[rdi+16]
Lvm_dispatch:
    lea rcx, [rip+Lvm_table]
    movsx rdx, word ptr [rcx+rax*2]
    add rdx, rcx
    jmp rdx
Lvm_const:
    sub eax, {constant_base}
    movss xmm0, [r13+rax*4]
Lvm_splat:
    shufps xmm0, xmm0, 0
Lvm_push:
    movaps [rdi], xmm0
    add rdi, 16
    jmp Lvm_next
Lvm_get:
    sub eax, 48
    shl eax, 4
    movaps xmm0, [rbx+rax]
    jmp Lvm_push
Lvm_set:
    movzx eax, byte ptr [rsi]
    inc rsi
    shl eax, 4
    sub rdi, 16
    movaps xmm0, [rdi]
    movaps [rbx+rax], xmm0
    jmp Lvm_next
Lvm_index:
    sub rdi, 16
    cvttss2si eax, [rdi]
    shl eax, 4
    movaps xmm0, [rbx+rax]
    jmp Lvm_push
Lvm_put:
    sub rdi, 32
    cvttss2si eax, [rdi]
    shl eax, 4
    movaps xmm0, [rdi+16]
    movaps [rbx+rax], xmm0
    jmp Lvm_next
Lvm_add:
    addps xmm0,xmm1
    jmp Lvm_push
Lvm_sub:
    subps xmm0,xmm1
    jmp Lvm_push
Lvm_mul:
    shufps xmm1,xmm1,0
    mulps xmm0,xmm1
    jmp Lvm_push
Lvm_div:
    shufps xmm1,xmm1,0
    divps xmm0,xmm1
    jmp Lvm_push
Lvm_neg:
    sub rdi,16
    xorps xmm0,xmm0
    subps xmm0,[rdi]
    jmp Lvm_push
Lvm_lt:
    cmpltss xmm0,xmm1
    jmp Lvm_bool
Lvm_eq:
    cmpeqss xmm0,xmm1
Lvm_bool:
    movd eax,xmm0
    and eax,1
    cvtsi2ss xmm0,eax
    jmp Lvm_splat
Lvm_jump:
    movzx eax,word ptr [rsi]
    lea rsi,[r12+rax]
    jmp Lvm_next
Lvm_zero:
    sub rdi,16
    xorps xmm0,xmm0
    ucomiss xmm0,[rdi]
    je Lvm_jump
    add rsi,2
    jmp Lvm_next
Lvm_call:
    movzx eax,byte ptr [rsi]
    inc rsi
    lea rdx,[r14+rax*4]
    mov [r15],rsi
    add r15,8
    movzx eax,word ptr [rdx]
    lea rsi,[r12+rax]
    movzx ecx,byte ptr [rdx+3]
    movzx eax,byte ptr [rdx+2]
    shl eax,4
    lea rdx,[rbx+rax]
    mov eax,ecx
    shl eax,4
    sub rdi,rax
    xor eax,eax
Lvm_args:
    movaps xmm0,[rdi+rax]
    movaps [rdx+rax],xmm0
    add eax,16
    dec ecx
    jnz Lvm_args
    jmp Lvm_next
Lvm_return:
    sub r15,8
    mov rsi,[r15]
    test rsi,rsi
    jnz Lvm_next
    add rsp,664
    pop r15
    pop r14
    pop r13
    pop r12
    pop rdi
    pop rsi
    pop rbp
    pop rbx
    ret
Lvm_vec:
    sub rdi,48
    movss xmm0,[rdi]
    movss xmm1,[rdi+16]
    unpcklps xmm0,xmm1
    movss xmm1,[rdi+32]
    movlhps xmm0,xmm1
    jmp Lvm_push
Lvm_sin:
    sub rdi,16
    fld dword ptr [rdi]
    fsin
    jmp Lvm_x87
Lvm_cos:
    sub rdi,16
    fld dword ptr [rdi]
    fcos
Lvm_x87:
    fstp dword ptr [rdi]
    movss xmm0,[rdi]
    jmp Lvm_splat
Lvm_abs:
    sub rdi,16
    mov eax,[rdi]
    and eax,0x7fffffff
    movd xmm0,eax
    jmp Lvm_splat
Lvm_sqrt:
    sub rdi,16
    sqrtss xmm0,[rdi]
    jmp Lvm_splat
Lvm_min:
    minss xmm0,xmm1
    jmp Lvm_splat
Lvm_max:
    maxss xmm0,xmm1
    jmp Lvm_splat
Lvm_dot:
    mulps xmm0,xmm1
    movaps xmm1,xmm0
    shufps xmm1,xmm1,0x55
    addss xmm1,xmm0
    shufps xmm0,xmm0,0xaa
    addss xmm0,xmm1
    jmp Lvm_splat
Lvm_cross:
    movaps xmm2,xmm0
    movaps xmm3,xmm1
    shufps xmm0,xmm0,0xc9
    shufps xmm1,xmm1,0xd2
    shufps xmm2,xmm2,0xd2
    shufps xmm3,xmm3,0xc9
    mulps xmm0,xmm1
    mulps xmm2,xmm3
    subps xmm0,xmm2
    jmp Lvm_push
Lvm_native:
    mov rcx,rbp
    mov rdx,rdi
    mov r8d,eax
    call {bridge}
    mov rdi,rax
    jmp Lvm_next
Lvm_pop:
    sub rdi,16
    jmp Lvm_next
Lvm_table:
    .short Lvm_set-Lvm_table
    .short Lvm_index-Lvm_table
    .short Lvm_put-Lvm_table
    .short Lvm_add-Lvm_table
    .short Lvm_sub-Lvm_table
    .short Lvm_mul-Lvm_table
    .short Lvm_div-Lvm_table
    .short Lvm_neg-Lvm_table
    .short Lvm_lt-Lvm_table
    .short Lvm_eq-Lvm_table
    .short Lvm_jump-Lvm_table
    .short Lvm_zero-Lvm_table
    .short Lvm_call-Lvm_table
    .short Lvm_return-Lvm_table
    .short Lvm_vec-Lvm_table
    .short Lvm_sin-Lvm_table
    .short Lvm_cos-Lvm_table
    .short Lvm_abs-Lvm_table
    .short Lvm_sqrt-Lvm_table
    .short Lvm_native-Lvm_table
    .short Lvm_native-Lvm_table
    .short Lvm_min-Lvm_table
    .short Lvm_max-Lvm_table
    .short Lvm_dot-Lvm_table
    .short Lvm_cross-Lvm_table
    .short Lvm_native-Lvm_table
    .short Lvm_pop-Lvm_table
    .short Lvm_native-Lvm_table
.seh_endproc
