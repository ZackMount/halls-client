.text
.globl memcpy
.globl memmove
.seh_proc memcpy
memcpy:
    push rdi
    .seh_pushreg rdi
    push rsi
    .seh_pushreg rsi
    .seh_endprologue
    mov rax, rcx
    mov rdi, rcx
    mov rsi, rdx
    mov rcx, r8
    rep movsb
    pop rsi
    pop rdi
    ret
.seh_endproc
.seh_proc memmove
memmove:
    push rdi
    .seh_pushreg rdi
    push rsi
    .seh_pushreg rsi
    .seh_endprologue
    mov rax, rcx
    mov rdi, rcx
    mov rsi, rdx
    mov rcx, r8
    cmp rdi, rsi
    jbe 2f
    lea rdx, [rsi + rcx]
    cmp rdi, rdx
    jae 2f
    lea rdi, [rdi + rcx - 1]
    lea rsi, [rsi + rcx - 1]
    std
    rep movsb
    cld
    jmp 3f
2:  rep movsb
3:  pop rsi
    pop rdi
    ret
.seh_endproc
.globl memset
.seh_proc memset
memset:
    push rdi
    .seh_pushreg rdi
    .seh_endprologue
    mov rdi, rcx
    mov r9, rcx
    mov eax, edx
    mov rcx, r8
    rep stosb
    mov rax, r9
    pop rdi
    ret
.seh_endproc
.globl memcmp
.seh_proc memcmp
memcmp:
    push rdi
    .seh_pushreg rdi
    push rsi
    .seh_pushreg rsi
    .seh_endprologue
    xor eax, eax
    test r8, r8
    jz 4f
    mov rdi, rcx
    mov rsi, rdx
    mov rcx, r8
    repe cmpsb
    je 4f
    movzx eax, byte ptr [rdi - 1]
    movzx ecx, byte ptr [rsi - 1]
    sub eax, ecx
4:  pop rsi
    pop rdi
    ret
.seh_endproc
.globl __chkstk
.seh_proc __chkstk
__chkstk:
    push rcx
    .seh_stackalloc 8
    push rax
    .seh_stackalloc 8
    .seh_endprologue
    lea rcx, [rsp + 24]
5:  cmp rax, 4096
    jb 6f
    sub rcx, 4096
    test byte ptr [rcx], al
    sub rax, 4096
    jmp 5b
6:  sub rcx, rax
    test byte ptr [rcx], al
    pop rax
    pop rcx
    ret
.seh_endproc
