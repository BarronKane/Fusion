.text

.globl fusion_linux_x86_64_context_swap
.type fusion_linux_x86_64_context_swap, @function
fusion_linux_x86_64_context_swap:
    mov [rdi + 0], rsp
    mov [rdi + 8], rbx
    mov [rdi + 16], rbp
    mov [rdi + 24], r12
    mov [rdi + 32], r13
    mov [rdi + 40], r14
    mov [rdi + 48], r15
    stmxcsr DWORD PTR [rdi + 56]
    fnstcw WORD PTR [rdi + 60]

    mov rsp, [rsi + 0]
    mov rbx, [rsi + 8]
    mov rbp, [rsi + 16]
    mov r12, [rsi + 24]
    mov r13, [rsi + 32]
    mov r14, [rsi + 40]
    mov r15, [rsi + 48]
    ldmxcsr DWORD PTR [rsi + 56]
    fldcw WORD PTR [rsi + 60]
    ret
.size fusion_linux_x86_64_context_swap, .-fusion_linux_x86_64_context_swap

.globl fusion_linux_x86_64_context_start
.type fusion_linux_x86_64_context_start, @function
fusion_linux_x86_64_context_start:
    mov rdi, r12
    mov rsi, r13
    jmp fusion_linux_x86_64_context_bootstrap
.size fusion_linux_x86_64_context_start, .-fusion_linux_x86_64_context_start
