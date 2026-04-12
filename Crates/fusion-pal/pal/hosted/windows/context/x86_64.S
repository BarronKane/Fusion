.text

.globl fusion_windows_x86_64_context_swap
fusion_windows_x86_64_context_swap:
    mov [rcx + 0], rsp
    mov [rcx + 8], rbx
    mov [rcx + 16], rbp
    mov [rcx + 24], rdi
    mov [rcx + 32], rsi
    mov [rcx + 40], r12
    mov [rcx + 48], r13
    mov [rcx + 56], r14
    mov [rcx + 64], r15
    stmxcsr DWORD PTR [rcx + 72]
    fnstcw WORD PTR [rcx + 76]
    movdqu XMMWORD PTR [rcx + 80], xmm6
    movdqu XMMWORD PTR [rcx + 96], xmm7
    movdqu XMMWORD PTR [rcx + 112], xmm8
    movdqu XMMWORD PTR [rcx + 128], xmm9
    movdqu XMMWORD PTR [rcx + 144], xmm10
    movdqu XMMWORD PTR [rcx + 160], xmm11
    movdqu XMMWORD PTR [rcx + 176], xmm12
    movdqu XMMWORD PTR [rcx + 192], xmm13
    movdqu XMMWORD PTR [rcx + 208], xmm14
    movdqu XMMWORD PTR [rcx + 224], xmm15

    mov rsp, [rdx + 0]
    mov rbx, [rdx + 8]
    mov rbp, [rdx + 16]
    mov rdi, [rdx + 24]
    mov rsi, [rdx + 32]
    mov r12, [rdx + 40]
    mov r13, [rdx + 48]
    mov r14, [rdx + 56]
    mov r15, [rdx + 64]
    ldmxcsr DWORD PTR [rdx + 72]
    fldcw WORD PTR [rdx + 76]
    movdqu xmm6, XMMWORD PTR [rdx + 80]
    movdqu xmm7, XMMWORD PTR [rdx + 96]
    movdqu xmm8, XMMWORD PTR [rdx + 112]
    movdqu xmm9, XMMWORD PTR [rdx + 128]
    movdqu xmm10, XMMWORD PTR [rdx + 144]
    movdqu xmm11, XMMWORD PTR [rdx + 160]
    movdqu xmm12, XMMWORD PTR [rdx + 176]
    movdqu xmm13, XMMWORD PTR [rdx + 192]
    movdqu xmm14, XMMWORD PTR [rdx + 208]
    movdqu xmm15, XMMWORD PTR [rdx + 224]
    ret

.globl fusion_windows_x86_64_context_start
fusion_windows_x86_64_context_start:
    mov rcx, r12
    mov rdx, r13
    jmp fusion_windows_x86_64_context_bootstrap
