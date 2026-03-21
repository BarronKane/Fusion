.syntax unified
.thumb

.global fusion_pal_cortex_m_call_inline_handler_on_reserved_stack
.type fusion_pal_cortex_m_call_inline_handler_on_reserved_stack, %function
.thumb_func
fusion_pal_cortex_m_call_inline_handler_on_reserved_stack:
    mrs r2, MSP
    sub r1, r1, #8
    str r2, [r1]
    str lr, [r1, #4]
    msr MSP, r1
    blx r0
    ldr r2, [sp]
    ldr lr, [sp, #4]
    msr MSP, r2
    bx lr
.size fusion_pal_cortex_m_call_inline_handler_on_reserved_stack, . - fusion_pal_cortex_m_call_inline_handler_on_reserved_stack
