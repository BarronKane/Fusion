.text

.globl _fusion_macos_x86_64_context_swap
_fusion_macos_x86_64_context_swap:
    # Save callee-saved general-purpose registers and control state into `from`.
    mov [rdi + 0], rsp
    mov [rdi + 8], rbx
    mov [rdi + 16], rbp
    mov [rdi + 24], r12
    mov [rdi + 32], r13
    mov [rdi + 40], r14
    mov [rdi + 48], r15
    stmxcsr DWORD PTR [rdi + 56]
    fnstcw WORD PTR [rdi + 60]

    # Restore the target context from `to`, then return into its saved instruction pointer.
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

.globl _fusion_macos_x86_64_context_start
_fusion_macos_x86_64_context_start:
    # The make() path seeds r12/r13 with entry + argument payload.
    mov rdi, r12
    mov rsi, r13
    jmp _fusion_macos_x86_64_context_bootstrap
