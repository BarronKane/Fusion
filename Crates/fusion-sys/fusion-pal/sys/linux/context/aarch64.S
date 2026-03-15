.text

.globl fusion_linux_aarch64_context_swap
.type fusion_linux_aarch64_context_swap, %function
fusion_linux_aarch64_context_swap:
    mov x9, sp
    str x9, [x0, #0]
    stp x19, x20, [x0, #8]
    stp x21, x22, [x0, #24]
    stp x23, x24, [x0, #40]
    stp x25, x26, [x0, #56]
    stp x27, x28, [x0, #72]
    stp x29, x30, [x0, #88]
    mrs x9, fpcr
    str x9, [x0, #104]
    mrs x9, fpsr
    str x9, [x0, #112]
    str d8, [x0, #120]
    str d9, [x0, #128]
    str d10, [x0, #136]
    str d11, [x0, #144]
    str d12, [x0, #152]
    str d13, [x0, #160]
    str d14, [x0, #168]
    str d15, [x0, #176]

    ldr x9, [x1, #0]
    mov sp, x9
    ldp x19, x20, [x1, #8]
    ldp x21, x22, [x1, #24]
    ldp x23, x24, [x1, #40]
    ldp x25, x26, [x1, #56]
    ldp x27, x28, [x1, #72]
    ldp x29, x30, [x1, #88]
    ldr x9, [x1, #104]
    msr fpcr, x9
    ldr x9, [x1, #112]
    msr fpsr, x9
    ldr d8, [x1, #120]
    ldr d9, [x1, #128]
    ldr d10, [x1, #136]
    ldr d11, [x1, #144]
    ldr d12, [x1, #152]
    ldr d13, [x1, #160]
    ldr d14, [x1, #168]
    ldr d15, [x1, #176]
    ret
.size fusion_linux_aarch64_context_swap, .-fusion_linux_aarch64_context_swap

.globl fusion_linux_aarch64_context_start
.type fusion_linux_aarch64_context_start, %function
fusion_linux_aarch64_context_start:
    mov x0, x19
    mov x1, x20
    b fusion_linux_aarch64_context_bootstrap
.size fusion_linux_aarch64_context_start, .-fusion_linux_aarch64_context_start
