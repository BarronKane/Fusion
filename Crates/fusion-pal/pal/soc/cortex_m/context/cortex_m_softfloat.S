.syntax unified
.thumb

.global fusion_cortex_m_context_swap
.type fusion_cortex_m_context_swap, %function
.thumb_func
fusion_cortex_m_context_swap:
    mov r2, sp
    str r2, [r0, #0]
    str r4, [r0, #4]
    str r5, [r0, #8]
    str r6, [r0, #12]
    str r7, [r0, #16]
    str r8, [r0, #20]
    str r9, [r0, #24]
    str r10, [r0, #28]
    str r11, [r0, #32]
    str lr, [r0, #36]

    ldr r2, [r1, #0]
    mov sp, r2
    ldr r4, [r1, #4]
    ldr r5, [r1, #8]
    ldr r6, [r1, #12]
    ldr r7, [r1, #16]
    ldr r8, [r1, #20]
    ldr r9, [r1, #24]
    ldr r10, [r1, #28]
    ldr r11, [r1, #32]
    ldr lr, [r1, #36]
    bx lr
.size fusion_cortex_m_context_swap, .-fusion_cortex_m_context_swap

.global fusion_cortex_m_context_start
.type fusion_cortex_m_context_start, %function
.thumb_func
fusion_cortex_m_context_start:
    mov r0, r4
    mov r1, r5
    b fusion_cortex_m_context_bootstrap
.size fusion_cortex_m_context_start, .-fusion_cortex_m_context_start
