//! AML method evaluation for the current proving subset.

use core::array;

use crate::aml::{
    AmlAccessWidth,
    AmlAddressSpaceId,
    AmlEncodedNameString,
    AmlError,
    AmlFieldAccessKind,
    AmlFieldDescriptor,
    AmlFieldUpdateKind,
    AmlExecutionPhase,
    AmlLoadedNamespace,
    AmlIntegerWidth,
    AmlMethodDescriptor,
    AmlNameSeg,
    AmlNamespaceNodePayload,
    AmlOpRegionDescriptor,
    AmlPkgLength,
    AmlRegionAccessHost,
    AmlResolvedNamePath,
    AmlResult,
    AmlRuntimeState,
    AmlValue,
};

/// One AML method invocation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmlMethodInvocation<'a> {
    pub method: crate::aml::AmlNamespaceNodeId,
    pub phase: AmlExecutionPhase,
    pub args: &'a [AmlValue<'a>],
}

/// Coarse AML evaluation outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmlEvaluationOutcome<'a> {
    pub return_value: Option<AmlValue<'a>>,
    pub blocked: bool,
}

/// Pure AML evaluator over one loaded namespace snapshot.
#[derive(Debug, Clone, Copy)]
pub struct AmlPureEvaluator<'records, 'blocks> {
    namespace: AmlLoadedNamespace<'records, 'blocks>,
}

impl<'records, 'blocks> AmlPureEvaluator<'records, 'blocks> {
    const MAX_RECURSION_DEPTH: u16 = 64;

    #[must_use]
    pub const fn new(namespace: AmlLoadedNamespace<'records, 'blocks>) -> Self {
        Self { namespace }
    }

    pub fn evaluate<'a>(
        &self,
        invocation: AmlMethodInvocation<'a>,
    ) -> AmlResult<AmlEvaluationOutcome<'a>>
    where
        'blocks: 'a,
    {
        self.evaluate_internal(None, None, invocation)
    }

    pub fn evaluate_with_state<'a>(
        &self,
        state: &AmlRuntimeState<'_>,
        invocation: AmlMethodInvocation<'a>,
    ) -> AmlResult<AmlEvaluationOutcome<'a>>
    where
        'blocks: 'a,
    {
        self.evaluate_internal(None, Some(state), invocation)
    }

    pub fn evaluate_with_host<'a>(
        &self,
        host: &dyn AmlRegionAccessHost,
        invocation: AmlMethodInvocation<'a>,
    ) -> AmlResult<AmlEvaluationOutcome<'a>>
    where
        'blocks: 'a,
    {
        self.evaluate_internal(Some(host), None, invocation)
    }

    pub fn evaluate_with_host_and_state<'a>(
        &self,
        host: &dyn AmlRegionAccessHost,
        state: &AmlRuntimeState<'_>,
        invocation: AmlMethodInvocation<'a>,
    ) -> AmlResult<AmlEvaluationOutcome<'a>>
    where
        'blocks: 'a,
    {
        self.evaluate_internal(Some(host), Some(state), invocation)
    }

    fn evaluate_internal<'a>(
        &self,
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        invocation: AmlMethodInvocation<'a>,
    ) -> AmlResult<AmlEvaluationOutcome<'a>>
    where
        'blocks: 'a,
    {
        let record = self
            .namespace
            .record(invocation.method)
            .ok_or_else(AmlError::undefined_object)?;
        let method = match record.payload {
            AmlNamespaceNodePayload::Method(method) => method,
            _ => return Err(AmlError::invalid_state()),
        };

        let body = self
            .namespace
            .code_bytes(method.body)
            .ok_or_else(AmlError::invalid_state)?;
        let scope_path = record
            .descriptor
            .path
            .parent()
            .unwrap_or_else(AmlResolvedNamePath::root);
        let integer_width = AmlIntegerWidth::from_definition_block_revision(
            self.namespace.blocks.dsdt.header.revision,
        );
        let mut frame = AmlEvalFrame::new(integer_width, scope_path, invocation.args, 0)?;

        match self.eval_term_list(body, host, state, invocation.phase, &mut frame)? {
            AmlControl::Continue => Ok(AmlEvaluationOutcome {
                return_value: None,
                blocked: false,
            }),
            AmlControl::Blocked => Ok(AmlEvaluationOutcome {
                return_value: None,
                blocked: true,
            }),
            AmlControl::Break => Err(AmlError::invalid_state()),
            AmlControl::Return(value) => Ok(AmlEvaluationOutcome {
                return_value: Some(value),
                blocked: false,
            }),
        }
    }

    fn eval_term_list<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<AmlControl<'a>>
    where
        'blocks: 'a,
    {
        let mut offset = 0_usize;
        while offset < bytes.len() {
            let (consumed, control) =
                self.eval_statement(&bytes[offset..], host, state, phase, frame)?;
            if consumed == 0 {
                return Err(AmlError::invalid_bytecode());
            }
            offset += consumed;
            if !matches!(control, AmlControl::Continue) {
                return Ok(control);
            }
        }
        Ok(AmlControl::Continue)
    }

    fn eval_statement<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)>
    where
        'blocks: 'a,
    {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x08 => self.eval_name_statement(bytes, host, state, phase, frame),
            0xA2 => self.eval_while(bytes, host, state, phase, frame),
            0xA3 => Ok((1, AmlControl::Continue)),
            0x70 => {
                let (value, value_consumed) =
                    self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
                let target_consumed = self.assign_target(
                    &bytes[1 + value_consumed..],
                    host,
                    state,
                    phase,
                    frame,
                    value.clone(),
                )?;
                Ok((1 + value_consumed + target_consumed, AmlControl::Continue))
            }
            0x5b => self.eval_ext_statement(bytes, host, state, phase, frame),
            0xA0 => self.eval_if(bytes, host, state, phase, frame),
            0xA5 => Ok((1, AmlControl::Break)),
            0xA4 => {
                let (value, consumed) =
                    self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
                Ok((1 + consumed, AmlControl::Return(value)))
            }
            _ => {
                let (_, consumed) = self.eval_term_arg(bytes, host, state, phase, frame)?;
                Ok((consumed, AmlControl::Continue))
            }
        }
    }

    fn eval_ext_statement<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)>
    where
        'blocks: 'a,
    {
        let sub = *bytes.get(1).ok_or_else(AmlError::truncated)?;
        match sub {
            0x80 => self.eval_dynamic_opregion_statement(bytes, host, state, phase, frame),
            0x81 => self.eval_dynamic_field_statement(bytes, host, state, phase, frame),
            0x21 => {
                let host = host.ok_or_else(AmlError::unsupported)?;
                let (microseconds, consumed) =
                    self.eval_term_arg(&bytes[2..], Some(host), state, phase, frame)?;
                host.stall_us(microseconds.as_integer()? as u32)?;
                Ok((2 + consumed, AmlControl::Continue))
            }
            0x22 => {
                let host = host.ok_or_else(AmlError::unsupported)?;
                let (milliseconds, consumed) =
                    self.eval_term_arg(&bytes[2..], Some(host), state, phase, frame)?;
                host.sleep_ms(milliseconds.as_integer()? as u32)?;
                Ok((2 + consumed, AmlControl::Continue))
            }
            0x23 => {
                let (target, target_consumed) = self.resolve_super_name(&bytes[2..], frame)?;
                let _timeout = bytes
                    .get(2 + target_consumed..2 + target_consumed + 2)
                    .ok_or_else(AmlError::truncated)?;
                let state = state.ok_or_else(AmlError::unsupported)?;
                let target = target.ok_or_else(AmlError::unsupported)?;
                if state.try_acquire_mutex(target)? {
                    Ok((4 + target_consumed, AmlControl::Continue))
                } else {
                    Ok((4 + target_consumed, AmlControl::Blocked))
                }
            }
            0x27 => {
                let (target, target_consumed) = self.resolve_super_name(&bytes[2..], frame)?;
                let state = state.ok_or_else(AmlError::unsupported)?;
                let target = target.ok_or_else(AmlError::unsupported)?;
                state.release_mutex(target)?;
                Ok((2 + target_consumed, AmlControl::Continue))
            }
            0x86 => {
                let host = host.ok_or_else(AmlError::unsupported)?;
                let (target, target_consumed) = self.resolve_super_name(&bytes[2..], frame)?;
                let target = target.ok_or_else(AmlError::unsupported)?;
                let (value, value_consumed) = self.eval_term_arg(
                    &bytes[2 + target_consumed..],
                    Some(host),
                    state,
                    phase,
                    frame,
                )?;
                host.notify(target, value.as_integer()? as u8)?;
                Ok((2 + target_consumed + value_consumed, AmlControl::Continue))
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn eval_if<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)>
    where
        'blocks: 'a,
    {
        let pkg = AmlPkgLength::parse(&bytes[1..])?;
        let object_end = 1 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let predicate_offset = 1 + usize::from(pkg.encoded_bytes);
        let (predicate, predicate_consumed) =
            self.eval_term_arg(&object_bytes[predicate_offset..], host, state, phase, frame)?;
        let body_start = predicate_offset + predicate_consumed;
        let mut consumed = object_end;
        let mut control = AmlControl::Continue;

        if predicate.as_logic() {
            control =
                self.eval_term_list(&object_bytes[body_start..], host, state, phase, frame)?;
            if !matches!(control, AmlControl::Continue) {
                return Ok((consumed, control));
            }
            if bytes.get(object_end) == Some(&0xA1) {
                let else_pkg = AmlPkgLength::parse(&bytes[object_end + 1..])?;
                consumed += 1 + else_pkg.value as usize;
            }
        } else if bytes.get(object_end) == Some(&0xA1) {
            let else_pkg = AmlPkgLength::parse(&bytes[object_end + 1..])?;
            let else_end = object_end + 1 + else_pkg.value as usize;
            let else_bytes = bytes
                .get(object_end..else_end)
                .ok_or_else(AmlError::truncated)?;
            let else_body_start = 1 + usize::from(else_pkg.encoded_bytes);
            control =
                self.eval_term_list(&else_bytes[else_body_start..], host, state, phase, frame)?;
            consumed = else_end;
        }

        Ok((consumed, control))
    }

    fn eval_name_statement<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)>
    where
        'blocks: 'a,
    {
        let encoded = AmlEncodedNameString::parse(&bytes[1..])?;
        let initializer_offset = 1 + usize::from(encoded.consumed_bytes);
        let (value, initializer_consumed) =
            self.eval_name_initializer(&bytes[initializer_offset..], host, state, phase, frame)?;
        let name = local_single_segment(encoded)?;
        frame.bind_named_value(name, value)?;
        Ok((
            initializer_offset + initializer_consumed,
            AmlControl::Continue,
        ))
    }

    fn eval_while<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)>
    where
        'blocks: 'a,
    {
        let pkg = AmlPkgLength::parse(&bytes[1..])?;
        let object_end = 1 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let predicate_offset = 1 + usize::from(pkg.encoded_bytes);
        loop {
            let (predicate, predicate_consumed) =
                self.eval_term_arg(&object_bytes[predicate_offset..], host, state, phase, frame)?;
            if !predicate.as_logic() {
                return Ok((object_end, AmlControl::Continue));
            }
            let body_start = predicate_offset + predicate_consumed;
            match self.eval_term_list(&object_bytes[body_start..], host, state, phase, frame)? {
                AmlControl::Continue => {}
                AmlControl::Break => return Ok((object_end, AmlControl::Continue)),
                control => return Ok((object_end, control)),
            }
        }
    }

    fn eval_term_arg<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x00 => Ok((AmlValue::integer(0, frame.integer_width), 1)),
            0x01 => Ok((AmlValue::integer(1, frame.integer_width), 1)),
            0xff => Ok((AmlValue::integer(u64::MAX, frame.integer_width), 1)),
            0x0a => Ok((
                AmlValue::integer(
                    u64::from(bytes.get(1).copied().ok_or_else(AmlError::truncated)?),
                    frame.integer_width,
                ),
                2,
            )),
            0x0b => {
                let raw = bytes.get(1..3).ok_or_else(AmlError::truncated)?;
                Ok((
                    AmlValue::integer(
                        u64::from(u16::from_le_bytes([raw[0], raw[1]])),
                        frame.integer_width,
                    ),
                    3,
                ))
            }
            0x0c => {
                let raw = bytes.get(1..5).ok_or_else(AmlError::truncated)?;
                Ok((
                    AmlValue::integer(
                        u64::from(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
                        frame.integer_width,
                    ),
                    5,
                ))
            }
            0x0e => {
                let raw = bytes.get(1..9).ok_or_else(AmlError::truncated)?;
                Ok((
                    AmlValue::integer(
                        u64::from_le_bytes([
                            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
                        ]),
                        frame.integer_width,
                    ),
                    9,
                ))
            }
            0x0d => {
                let Some(length) = bytes[1..].iter().position(|byte| *byte == 0) else {
                    return Err(AmlError::truncated());
                };
                let raw = bytes.get(1..1 + length).ok_or_else(AmlError::truncated)?;
                let value = core::str::from_utf8(raw).map_err(|_| AmlError::invalid_bytecode())?;
                Ok((AmlValue::String(value), 2 + length))
            }
            0x60..=0x67 => {
                let index = usize::from(opcode - 0x60);
                let value = frame.locals[index]
                    .clone()
                    .ok_or_else(AmlError::invalid_state)?;
                Ok((value, 1))
            }
            0x68..=0x6e => {
                let index = usize::from(opcode - 0x68);
                let value = frame.args[index]
                    .clone()
                    .ok_or_else(AmlError::invalid_state)?;
                Ok((value, 1))
            }
            0x70 => {
                let (value, value_consumed) =
                    self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
                let target_consumed = self.assign_target(
                    &bytes[1 + value_consumed..],
                    host,
                    state,
                    phase,
                    frame,
                    value.clone(),
                )?;
                Ok((value, 1 + value_consumed + target_consumed))
            }
            0x72 => self.eval_binary_op(bytes, host, state, phase, frame, |lhs, rhs, width| {
                AmlValue::integer(lhs.wrapping_add(rhs), width)
            }),
            0x74 => self.eval_binary_op(bytes, host, state, phase, frame, |lhs, rhs, width| {
                AmlValue::integer(lhs.wrapping_sub(rhs), width)
            }),
            0x77 => self.eval_binary_op(bytes, host, state, phase, frame, |lhs, rhs, width| {
                AmlValue::integer(lhs.wrapping_mul(rhs), width)
            }),
            0x78 => self.eval_divide(bytes, host, state, phase, frame),
            0x75 => self.eval_update_op(bytes, host, state, frame, |value, width| {
                AmlValue::integer(value.wrapping_add(1), width)
            }),
            0x76 => self.eval_update_op(bytes, host, state, frame, |value, width| {
                AmlValue::integer(value.wrapping_sub(1), width)
            }),
            0x79 => self.eval_binary_op(bytes, host, state, phase, frame, |lhs, rhs, width| {
                AmlValue::integer(lhs.wrapping_shl(rhs as u32), width)
            }),
            0x7a => self.eval_binary_op(bytes, host, state, phase, frame, |lhs, rhs, width| {
                AmlValue::integer(lhs.wrapping_shr(rhs as u32), width)
            }),
            0x7b => self.eval_binary_op(bytes, host, state, phase, frame, |lhs, rhs, width| {
                AmlValue::integer(lhs & rhs, width)
            }),
            0x7d => self.eval_binary_op(bytes, host, state, phase, frame, |lhs, rhs, width| {
                AmlValue::integer(lhs | rhs, width)
            }),
            0x7f => self.eval_binary_op(bytes, host, state, phase, frame, |lhs, rhs, width| {
                AmlValue::integer(lhs ^ rhs, width)
            }),
            0x80 => {
                let (value, consumed) =
                    self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
                Ok((
                    AmlValue::integer(!value.as_integer()?, frame.integer_width),
                    1 + consumed,
                ))
            }
            0x83 => self.eval_deref(bytes, host, state, phase, frame),
            0x87 => self.eval_size_of(bytes, host, state, phase, frame),
            0x88 => self.eval_index(bytes, host, state, phase, frame),
            0x8e => self.eval_object_type(bytes, host, state, phase, frame),
            0x92 => {
                let (value, consumed) =
                    self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
                Ok((
                    AmlValue::integer((!value.as_logic()) as u64, frame.integer_width),
                    1 + consumed,
                ))
            }
            0x93 => {
                self.eval_logic_compare(bytes, host, state, phase, frame, |lhs, rhs| lhs == rhs)
            }
            0x94 => self.eval_logic_compare(bytes, host, state, phase, frame, |lhs, rhs| lhs > rhs),
            0x95 => self.eval_logic_compare(bytes, host, state, phase, frame, |lhs, rhs| lhs < rhs),
            b'\\' | b'^' | b'_' | b'A'..=b'Z' => {
                self.eval_named_term(bytes, host, state, phase, frame)
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn eval_update_op<'a, F>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        frame: &mut AmlEvalFrame<'a>,
        op: F,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
        F: FnOnce(u64, AmlIntegerWidth) -> AmlValue<'a>,
    {
        let (value, target_consumed) = self.read_target_value(&bytes[1..], host, state, frame)?;
        let result = op(value.as_integer()?, frame.integer_width);
        self.assign_target(
            &bytes[1..],
            host,
            state,
            AmlExecutionPhase::Runtime,
            frame,
            result.clone(),
        )?;
        Ok((result, 1 + target_consumed))
    }

    fn eval_named_term<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let encoded = AmlEncodedNameString::parse(bytes)?;
        if let Some(name) = local_single_segment_if_present(encoded) {
            if let Some(value) = frame.named_value(name) {
                return Ok((value.clone(), usize::from(encoded.consumed_bytes)));
            }
            if let Some(field) = frame.named_field(name) {
                let host = host.ok_or_else(AmlError::unsupported)?;
                let value = self.read_dynamic_field_value(host, field)?;
                return Ok((
                    AmlValue::integer(value, frame.integer_width),
                    usize::from(encoded.consumed_bytes),
                ));
            }
        }
        let path = self
            .namespace
            .resolve_lookup_path(frame.current_scope_path, encoded)?;
        let record = self
            .namespace
            .record_by_path(path)
            .ok_or_else(AmlError::undefined_object)?;
        match record.payload {
            AmlNamespaceNodePayload::NameInteger(value) => Ok((
                AmlValue::integer(
                    state
                        .and_then(|state| state.read_integer(record.descriptor.id))
                        .unwrap_or(value),
                    frame.integer_width,
                ),
                usize::from(encoded.consumed_bytes),
            )),
            AmlNamespaceNodePayload::None
                if record.descriptor.kind == crate::aml::AmlObjectKind::Name =>
            {
                let body = record.body.ok_or_else(AmlError::invalid_state)?;
                let (value, _) = self.eval_static_name_value(body, host, state, phase, frame)?;
                Ok((value, usize::from(encoded.consumed_bytes)))
            }
            AmlNamespaceNodePayload::Field(field) => {
                let host = host.ok_or_else(AmlError::unsupported)?;
                let value = self.read_field_value(host, field)?;
                Ok((
                    AmlValue::integer(value, frame.integer_width),
                    usize::from(encoded.consumed_bytes),
                ))
            }
            AmlNamespaceNodePayload::Method(method) => {
                let (value, args_consumed) = self.invoke_method_from_term(
                    method,
                    &bytes[usize::from(encoded.consumed_bytes)..],
                    host,
                    state,
                    phase,
                    frame,
                )?;
                Ok((value, usize::from(encoded.consumed_bytes) + args_consumed))
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn eval_binary_op<'a, F>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
        op: F,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
        F: FnOnce(u64, u64, AmlIntegerWidth) -> AmlValue<'a>,
    {
        let (lhs, lhs_consumed) = self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
        let (rhs, rhs_consumed) =
            self.eval_term_arg(&bytes[1 + lhs_consumed..], host, state, phase, frame)?;
        let result = op(lhs.as_integer()?, rhs.as_integer()?, frame.integer_width);
        let target_consumed = self.assign_target(
            &bytes[1 + lhs_consumed + rhs_consumed..],
            host,
            state,
            phase,
            frame,
            result.clone(),
        )?;
        Ok((result, 1 + lhs_consumed + rhs_consumed + target_consumed))
    }

    fn eval_logic_compare<'a, F>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
        predicate: F,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
        F: FnOnce(u64, u64) -> bool,
    {
        let (lhs, lhs_consumed) = self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
        let (rhs, rhs_consumed) =
            self.eval_term_arg(&bytes[1 + lhs_consumed..], host, state, phase, frame)?;
        Ok((
            AmlValue::integer(
                predicate(lhs.as_integer()?, rhs.as_integer()?) as u64,
                frame.integer_width,
            ),
            1 + lhs_consumed + rhs_consumed,
        ))
    }

    fn eval_divide<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let (lhs, lhs_consumed) = self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
        let (rhs, rhs_consumed) =
            self.eval_term_arg(&bytes[1 + lhs_consumed..], host, state, phase, frame)?;
        let divisor = rhs.as_integer()?;
        if divisor == 0 {
            return Err(AmlError::invalid_state());
        }
        let dividend = lhs.as_integer()?;
        let remainder = AmlValue::integer(dividend % divisor, frame.integer_width);
        let quotient = AmlValue::integer(dividend / divisor, frame.integer_width);
        let remainder_target_consumed = self.assign_target(
            &bytes[1 + lhs_consumed + rhs_consumed..],
            host,
            state,
            phase,
            frame,
            remainder.clone(),
        )?;
        let quotient_target_consumed = self.assign_target(
            &bytes[1 + lhs_consumed + rhs_consumed + remainder_target_consumed..],
            host,
            state,
            phase,
            frame,
            quotient.clone(),
        )?;
        Ok((
            quotient,
            1 + lhs_consumed + rhs_consumed + remainder_target_consumed + quotient_target_consumed,
        ))
    }

    fn eval_deref<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let (value, consumed) = self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
        Ok((value, 1 + consumed))
    }

    fn eval_size_of<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let (value, consumed) = self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
        let len = self.value_size(value, state)? as u64;
        Ok((AmlValue::integer(len, frame.integer_width), 1 + consumed))
    }

    fn eval_object_type<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let (value, consumed) = self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
        let kind = self.object_type_id(value);
        Ok((AmlValue::integer(kind, frame.integer_width), 1 + consumed))
    }

    fn eval_index<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let (base, base_consumed) = self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
        let (index, index_consumed) =
            self.eval_term_arg(&bytes[1 + base_consumed..], host, state, phase, frame)?;
        let value = self.index_value(base, index.as_integer()?, host, state, phase, frame)?;
        let target_consumed = self.assign_target(
            &bytes[1 + base_consumed + index_consumed..],
            host,
            state,
            phase,
            frame,
            value.clone(),
        )?;
        Ok((value, 1 + base_consumed + index_consumed + target_consumed))
    }

    fn invoke_method_from_term<'a>(
        &self,
        method: AmlMethodDescriptor,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        if frame.recursion_depth >= Self::MAX_RECURSION_DEPTH {
            return Err(AmlError::invalid_state());
        }

        let mut consumed = 0_usize;
        let mut args: [Option<AmlValue<'a>>; 7] = array::from_fn(|_| None);
        let mut index = 0_u8;
        while index < method.arg_count {
            let (value, arg_consumed) =
                self.eval_term_arg(&bytes[consumed..], host, state, phase, frame)?;
            args[usize::from(index)] = Some(value);
            consumed += arg_consumed;
            index += 1;
        }

        let args_len = usize::from(method.arg_count);
        let mut arg_values: [AmlValue<'a>; 7] = array::from_fn(|_| AmlValue::None);
        let mut arg_index = 0_usize;
        while arg_index < args_len {
            arg_values[arg_index] = args[arg_index]
                .clone()
                .ok_or_else(AmlError::invalid_state)?;
            arg_index += 1;
        }
        let return_value = self
            .invoke_method_descriptor(
                method,
                &arg_values[..args_len],
                host,
                state,
                phase,
                frame.recursion_depth + 1,
            )?
            .unwrap_or(AmlValue::None);
        Ok((return_value, consumed))
    }

    fn invoke_method_descriptor<'a>(
        &self,
        method: AmlMethodDescriptor,
        args: &[AmlValue<'a>],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        recursion_depth: u16,
    ) -> AmlResult<Option<AmlValue<'a>>>
    where
        'blocks: 'a,
    {
        let record = self
            .namespace
            .record(method.node)
            .ok_or_else(AmlError::undefined_object)?;
        let body = self
            .namespace
            .code_bytes(method.body)
            .ok_or_else(AmlError::invalid_state)?;
        let scope_path = record
            .descriptor
            .path
            .parent()
            .unwrap_or_else(AmlResolvedNamePath::root);
        let mut frame = AmlEvalFrame::new(
            AmlIntegerWidth::from_definition_block_revision(
                self.namespace.blocks.dsdt.header.revision,
            ),
            scope_path,
            args,
            recursion_depth,
        )?;

        match self.eval_term_list(body, host, state, phase, &mut frame)? {
            AmlControl::Continue => Ok(None),
            AmlControl::Blocked => Err(AmlError::unsupported()),
            AmlControl::Break => Err(AmlError::invalid_state()),
            AmlControl::Return(value) => Ok(Some(value.clone())),
        }
    }

    fn assign_target<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
        value: AmlValue<'a>,
    ) -> AmlResult<usize>
    where
        'blocks: 'a,
    {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x00 => Ok(1),
            0x60..=0x67 => {
                frame.locals[usize::from(opcode - 0x60)] = Some(value);
                Ok(1)
            }
            0x88 => self.assign_index_target(bytes, host, state, phase, frame, value),
            0x68..=0x6e => {
                frame.args[usize::from(opcode - 0x68)] = Some(value);
                Ok(1)
            }
            b'\\' | b'^' | b'_' | b'A'..=b'Z' => {
                let encoded = AmlEncodedNameString::parse(bytes)?;
                if let Some(name) = local_single_segment_if_present(encoded) {
                    if let Some(AmlValue::BufferHandle(handle)) = frame.named_value(name).cloned() {
                        let state = state.ok_or_else(AmlError::unsupported)?;
                        self.copy_value_into_buffer(state, handle, value.clone())?;
                        return Ok(usize::from(encoded.consumed_bytes));
                    }
                    if let Some(field) = frame.named_field(name) {
                        let host = host.ok_or_else(AmlError::unsupported)?;
                        self.write_dynamic_field_value(host, field, value.as_integer()?)?;
                        return Ok(usize::from(encoded.consumed_bytes));
                    }
                    if frame.write_named_value(name, value.clone()) {
                        return Ok(usize::from(encoded.consumed_bytes));
                    }
                }
                let path = self
                    .namespace
                    .resolve_lookup_path(frame.current_scope_path, encoded)?;
                self.write_named_target(path, host, state, value)?;
                Ok(usize::from(encoded.consumed_bytes))
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn read_target_value<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x60..=0x67 => {
                let value = frame.locals[usize::from(opcode - 0x60)]
                    .clone()
                    .ok_or_else(AmlError::invalid_state)?;
                Ok((value, 1))
            }
            0x68..=0x6e => {
                let value = frame.args[usize::from(opcode - 0x68)]
                    .clone()
                    .ok_or_else(AmlError::invalid_state)?;
                Ok((value, 1))
            }
            b'\\' | b'^' | b'_' | b'A'..=b'Z' => {
                self.eval_named_term(bytes, host, state, AmlExecutionPhase::Runtime, frame)
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn resolve_super_name(
        &self,
        bytes: &[u8],
        frame: &AmlEvalFrame<'_>,
    ) -> AmlResult<(Option<crate::aml::AmlNamespaceNodeId>, usize)> {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x60..=0x67 | 0x68..=0x6e => Ok((None, 1)),
            0x5b => {
                let sub = *bytes.get(1).ok_or_else(AmlError::truncated)?;
                match sub {
                    0x31 => Ok((None, 2)),
                    _ => Err(AmlError::unsupported()),
                }
            }
            b'\\' | b'^' | b'_' | b'A'..=b'Z' => {
                let encoded = AmlEncodedNameString::parse(bytes)?;
                let path = self
                    .namespace
                    .resolve_lookup_path(frame.current_scope_path, encoded)?;
                let record = self
                    .namespace
                    .record_by_path(path)
                    .ok_or_else(AmlError::undefined_object)?;
                Ok((
                    Some(record.descriptor.id),
                    usize::from(encoded.consumed_bytes),
                ))
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn write_named_target<'a>(
        &self,
        path: AmlResolvedNamePath,
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        value: AmlValue<'a>,
    ) -> AmlResult<()> {
        let record = self
            .namespace
            .record_by_path(path)
            .ok_or_else(AmlError::undefined_object)?;
        match record.payload {
            AmlNamespaceNodePayload::Field(field) => {
                let host = host.ok_or_else(AmlError::unsupported)?;
                self.write_field_value(host, field, value.as_integer()?)
            }
            AmlNamespaceNodePayload::NameInteger(_) => {
                let state = state.ok_or_else(AmlError::unsupported)?;
                state.write_integer(record.descriptor.id, value.as_integer()?)
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn assign_index_target<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
        value: AmlValue<'a>,
    ) -> AmlResult<usize>
    where
        'blocks: 'a,
    {
        let state = state.ok_or_else(AmlError::unsupported)?;
        let (base, base_consumed) =
            self.eval_term_arg(&bytes[1..], host, Some(state), phase, frame)?;
        let (index, index_consumed) =
            self.eval_term_arg(&bytes[1 + base_consumed..], host, Some(state), phase, frame)?;
        let target_consumed = self.assign_target(
            &bytes[1 + base_consumed + index_consumed..],
            host,
            Some(state),
            phase,
            frame,
            AmlValue::None,
        )?;
        let index = u8::try_from(index.as_integer()?).map_err(|_| AmlError::overflow())?;
        match base {
            AmlValue::PackageHandle(handle) => {
                state.write_package_value(handle, index, self.runtime_aggregate_value(value)?)?;
            }
            AmlValue::BufferHandle(handle) => {
                state.write_buffer_byte(handle, index, value.as_integer()? as u8)?;
            }
            _ => return Err(AmlError::unsupported()),
        }
        Ok(1 + base_consumed + index_consumed + target_consumed)
    }

    fn eval_name_initializer<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x0d => self.eval_term_arg(bytes, host, state, phase, frame),
            0x11 => self.eval_buffer_initializer(bytes, host, state, phase, frame),
            0x12 => self.eval_package_initializer(bytes, host, state, phase, frame),
            _ => Err(AmlError::unsupported()),
        }
    }

    fn eval_package_initializer<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let state = state.ok_or_else(AmlError::unsupported)?;
        let pkg = AmlPkgLength::parse(&bytes[1..])?;
        let object_end = 1 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let count_offset = 1 + usize::from(pkg.encoded_bytes);
        let element_count = *object_bytes
            .get(count_offset)
            .ok_or_else(AmlError::truncated)?;
        let handle = state.create_package(element_count)?;
        let mut cursor = count_offset + 1;
        let mut index = 0_u8;
        while index < element_count {
            if cursor >= object_end {
                break;
            }
            let (value, consumed) =
                self.eval_term_arg(&object_bytes[cursor..], host, Some(state), phase, frame)?;
            state.write_package_value(handle, index, self.runtime_aggregate_value(value)?)?;
            cursor += consumed;
            index += 1;
        }
        Ok((AmlValue::PackageHandle(handle), object_end))
    }

    fn eval_buffer_initializer<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let state = state.ok_or_else(AmlError::unsupported)?;
        let pkg = AmlPkgLength::parse(&bytes[1..])?;
        let object_end = 1 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let size_offset = 1 + usize::from(pkg.encoded_bytes);
        let (len, len_consumed) = self.eval_term_arg(
            &object_bytes[size_offset..],
            host,
            Some(state),
            phase,
            frame,
        )?;
        let len = u8::try_from(len.as_integer()?).map_err(|_| AmlError::overflow())?;
        let handle = state.create_buffer(len)?;
        let init_start = size_offset + len_consumed;
        let init_bytes = object_bytes
            .get(init_start..object_end)
            .ok_or_else(AmlError::truncated)?;
        state.copy_bytes_into_buffer(handle, init_bytes)?;
        Ok((AmlValue::BufferHandle(handle), object_end))
    }

    fn eval_static_name_value<'a>(
        &self,
        location: crate::aml::AmlCodeLocation,
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        'blocks: 'a,
    {
        let bytes = self
            .namespace
            .code_bytes(location)
            .ok_or_else(AmlError::invalid_state)?;
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x00 => Ok((AmlValue::integer(0, frame.integer_width), 1)),
            0x01 => Ok((AmlValue::integer(1, frame.integer_width), 1)),
            0xff => Ok((AmlValue::integer(u64::MAX, frame.integer_width), 1)),
            0x0a => Ok((
                AmlValue::integer(
                    u64::from(*bytes.get(1).ok_or_else(AmlError::truncated)?),
                    frame.integer_width,
                ),
                2,
            )),
            0x0b => {
                let raw = bytes.get(1..3).ok_or_else(AmlError::truncated)?;
                Ok((
                    AmlValue::integer(
                        u64::from(u16::from_le_bytes([raw[0], raw[1]])),
                        frame.integer_width,
                    ),
                    3,
                ))
            }
            0x0c => {
                let raw = bytes.get(1..5).ok_or_else(AmlError::truncated)?;
                Ok((
                    AmlValue::integer(
                        u64::from(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
                        frame.integer_width,
                    ),
                    5,
                ))
            }
            0x0e => {
                let raw = bytes.get(1..9).ok_or_else(AmlError::truncated)?;
                Ok((
                    AmlValue::integer(
                        u64::from_le_bytes([
                            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
                        ]),
                        frame.integer_width,
                    ),
                    9,
                ))
            }
            0x0d => {
                let Some(length) = bytes[1..].iter().position(|byte| *byte == 0) else {
                    return Err(AmlError::truncated());
                };
                Ok((
                    AmlValue::StaticString(crate::aml::AmlCodeLocation {
                        block_index: location.block_index,
                        span: crate::aml::AmlBytecodeSpan {
                            offset: location.span.offset + 1,
                            length: length as u32,
                        },
                    }),
                    2 + length,
                ))
            }
            0x11 => {
                let pkg = AmlPkgLength::parse(&bytes[1..])?;
                let object_end = 1 + pkg.value as usize;
                let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
                let size_offset = 1 + usize::from(pkg.encoded_bytes);
                let (len, len_consumed) =
                    self.eval_term_arg(&object_bytes[size_offset..], host, state, phase, frame)?;
                let len = usize::try_from(len.as_integer()?).map_err(|_| AmlError::overflow())?;
                let data_start = size_offset + len_consumed;
                let raw = object_bytes
                    .get(data_start..object_end)
                    .ok_or_else(AmlError::truncated)?;
                let len = core::cmp::min(len, raw.len());
                Ok((AmlValue::Buffer(&raw[..len]), object_end))
            }
            0x12 => {
                let pkg = AmlPkgLength::parse(&bytes[1..])?;
                let object_end = 1 + pkg.value as usize;
                Ok((
                    AmlValue::StaticPackage(crate::aml::AmlCodeLocation {
                        block_index: location.block_index,
                        span: crate::aml::AmlBytecodeSpan {
                            offset: location.span.offset,
                            length: object_end as u32,
                        },
                    }),
                    object_end,
                ))
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn runtime_aggregate_value<'a>(
        &self,
        value: AmlValue<'a>,
    ) -> AmlResult<crate::aml::AmlRuntimeAggregateValue> {
        match value {
            AmlValue::Integer(value) => Ok(crate::aml::AmlRuntimeAggregateValue::Integer(value)),
            AmlValue::BufferHandle(handle) => {
                Ok(crate::aml::AmlRuntimeAggregateValue::Buffer(handle))
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn copy_value_into_buffer<'a>(
        &self,
        state: &AmlRuntimeState<'_>,
        handle: crate::aml::AmlRuntimeBufferHandle,
        value: AmlValue<'a>,
    ) -> AmlResult<()> {
        match value {
            AmlValue::String(value) => state.copy_bytes_into_buffer(handle, value.as_bytes()),
            AmlValue::StaticString(location) => {
                let bytes = self
                    .namespace
                    .code_bytes(location)
                    .ok_or_else(AmlError::invalid_state)?;
                state.copy_bytes_into_buffer(handle, bytes)
            }
            AmlValue::Buffer(value) => state.copy_bytes_into_buffer(handle, value),
            AmlValue::BufferHandle(source) => {
                let len = state
                    .read_buffer_len(source)
                    .ok_or_else(AmlError::invalid_state)?;
                let mut bytes = [0_u8; crate::aml::AML_MAX_BUFFER_BYTES];
                let mut index = 0_u8;
                while index < len {
                    bytes[usize::from(index)] = state
                        .read_buffer_byte(source, index)
                        .ok_or_else(AmlError::invalid_state)?;
                    index += 1;
                }
                state.copy_bytes_into_buffer(handle, &bytes[..usize::from(len)])
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn value_size(
        &self,
        value: AmlValue<'_>,
        state: Option<&AmlRuntimeState<'_>>,
    ) -> AmlResult<usize> {
        match value {
            AmlValue::String(value) => Ok(value.len()),
            AmlValue::StaticString(location) => self
                .namespace
                .code_bytes(location)
                .map(|bytes| bytes.len())
                .ok_or_else(AmlError::invalid_state),
            AmlValue::Buffer(value) => Ok(value.len()),
            AmlValue::BufferHandle(handle) => state
                .and_then(|state| state.read_buffer_len(handle))
                .map(usize::from)
                .ok_or_else(AmlError::unsupported),
            AmlValue::Package(value) => Ok(value.len()),
            AmlValue::StaticPackage(location) => Ok(usize::from(static_package_element_count(
                self.namespace
                    .code_bytes(location)
                    .ok_or_else(AmlError::invalid_state)?,
            )?)),
            AmlValue::PackageHandle(handle) => state
                .and_then(|state| state.read_package_len(handle))
                .map(usize::from)
                .ok_or_else(AmlError::unsupported),
            _ => Err(AmlError::unsupported()),
        }
    }

    fn object_type_id(&self, value: AmlValue<'_>) -> u64 {
        match value {
            AmlValue::Integer(_) => 0x01,
            AmlValue::String(_) | AmlValue::StaticString(_) => 0x02,
            AmlValue::Buffer(_) | AmlValue::BufferHandle(_) => 0x03,
            AmlValue::Package(_) | AmlValue::StaticPackage(_) | AmlValue::PackageHandle(_) => 0x04,
            AmlValue::DebugObject => 0x10,
            AmlValue::None => 0,
        }
    }

    fn index_value<'a>(
        &self,
        base: AmlValue<'a>,
        index: u64,
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<AmlValue<'a>>
    where
        'blocks: 'a,
    {
        let index = u8::try_from(index).map_err(|_| AmlError::overflow())?;
        match base {
            AmlValue::PackageHandle(handle) => {
                let state = state.ok_or_else(AmlError::unsupported)?;
                match state
                    .read_package_value(handle, index)
                    .ok_or_else(AmlError::invalid_state)?
                {
                    crate::aml::AmlRuntimeAggregateValue::Integer(value) => {
                        Ok(AmlValue::integer(value, frame.integer_width))
                    }
                    crate::aml::AmlRuntimeAggregateValue::Buffer(handle) => {
                        Ok(AmlValue::BufferHandle(handle))
                    }
                    crate::aml::AmlRuntimeAggregateValue::None => Ok(AmlValue::None),
                }
            }
            AmlValue::StaticPackage(location) => {
                self.static_package_element_value(location, index, host, state, phase, frame)
            }
            AmlValue::BufferHandle(handle) => {
                let state = state.ok_or_else(AmlError::unsupported)?;
                let value = state
                    .read_buffer_byte(handle, index)
                    .ok_or_else(AmlError::invalid_state)?;
                Ok(AmlValue::integer(u64::from(value), frame.integer_width))
            }
            AmlValue::Buffer(bytes) => {
                let value = *bytes
                    .get(usize::from(index))
                    .ok_or_else(AmlError::invalid_state)?;
                Ok(AmlValue::integer(u64::from(value), frame.integer_width))
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn static_package_element_value<'a>(
        &self,
        location: crate::aml::AmlCodeLocation,
        index: u8,
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<AmlValue<'a>>
    where
        'blocks: 'a,
    {
        let bytes = self
            .namespace
            .code_bytes(location)
            .ok_or_else(AmlError::invalid_state)?;
        let pkg = AmlPkgLength::parse(&bytes[1..])?;
        let object_end = 1 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let count_offset = 1 + usize::from(pkg.encoded_bytes);
        let element_count = *object_bytes
            .get(count_offset)
            .ok_or_else(AmlError::truncated)?;
        if index >= element_count {
            return Err(AmlError::invalid_state());
        }
        let mut cursor = count_offset + 1;
        let mut current = 0_u8;
        while current < element_count {
            let (value, consumed) = self.eval_static_name_value(
                crate::aml::AmlCodeLocation {
                    block_index: location.block_index,
                    span: crate::aml::AmlBytecodeSpan {
                        offset: location.span.offset + cursor as u32,
                        length: (object_end - cursor) as u32,
                    },
                },
                host,
                state,
                phase,
                frame,
            )?;
            if current == index {
                return Ok(value);
            }
            cursor += consumed;
            current += 1;
        }
        Err(AmlError::truncated())
    }

    fn read_field_value(
        &self,
        host: &dyn AmlRegionAccessHost,
        field: AmlFieldDescriptor,
    ) -> AmlResult<u64> {
        let region = self.resolve_region(field)?;
        self.read_field_bits(host, region, u64::from(field.bit_offset), field.bit_width)
    }

    fn read_dynamic_field_value(
        &self,
        host: &dyn AmlRegionAccessHost,
        field: AmlDynamicFieldBinding,
    ) -> AmlResult<u64> {
        self.read_field_bits(
            host,
            field.region,
            u64::from(field.bit_offset),
            field.bit_width,
        )
    }

    fn read_field_bits(
        &self,
        host: &dyn AmlRegionAccessHost,
        region: AmlOpRegionDescriptor,
        start_bit: u64,
        bit_width: u32,
    ) -> AmlResult<u64> {
        if bit_width == 0 || bit_width > 64 {
            return Err(AmlError::unsupported());
        }
        let end_bit = start_bit
            .checked_add(u64::from(bit_width))
            .ok_or_else(AmlError::overflow)?;
        let first_byte = start_bit / 8;
        let last_byte = end_bit.saturating_sub(1) / 8;
        self.ensure_region_byte_range(region, first_byte, last_byte)?;

        let mut aggregate = 0_u64;
        let byte_count = last_byte
            .checked_sub(first_byte)
            .and_then(|delta| delta.checked_add(1))
            .ok_or_else(AmlError::overflow)?;
        let mut index = 0_u64;
        while index < byte_count {
            let byte = self.read_region_byte(host, region, first_byte + index)?;
            aggregate |= u64::from(byte) << (index * 8);
            index += 1;
        }

        let shift = start_bit % 8;
        Ok((aggregate >> shift) & width_mask(bit_width))
    }

    fn write_field_value(
        &self,
        host: &dyn AmlRegionAccessHost,
        field: AmlFieldDescriptor,
        value: u64,
    ) -> AmlResult<()> {
        let region = self.resolve_region(field)?;
        self.write_field_bits(
            host,
            region,
            u64::from(field.bit_offset),
            field.bit_width,
            field.update,
            value,
        )
    }

    fn write_dynamic_field_value(
        &self,
        host: &dyn AmlRegionAccessHost,
        field: AmlDynamicFieldBinding,
        value: u64,
    ) -> AmlResult<()> {
        self.write_field_bits(
            host,
            field.region,
            u64::from(field.bit_offset),
            field.bit_width,
            field.update,
            value,
        )
    }

    fn write_field_bits(
        &self,
        host: &dyn AmlRegionAccessHost,
        region: AmlOpRegionDescriptor,
        start_bit: u64,
        bit_width: u32,
        update: AmlFieldUpdateKind,
        value: u64,
    ) -> AmlResult<()> {
        if bit_width == 0 || bit_width > 64 {
            return Err(AmlError::unsupported());
        }
        let end_bit = start_bit
            .checked_add(u64::from(bit_width))
            .ok_or_else(AmlError::overflow)?;
        let first_byte = start_bit / 8;
        let last_byte = end_bit.saturating_sub(1) / 8;
        self.ensure_region_byte_range(region, first_byte, last_byte)?;

        let masked_value = value & width_mask(bit_width);
        let shifted_value = masked_value
            .checked_shl((start_bit % 8) as u32)
            .unwrap_or(0);
        let byte_count = last_byte
            .checked_sub(first_byte)
            .and_then(|delta| delta.checked_add(1))
            .ok_or_else(AmlError::overflow)?;
        let mut index = 0_u64;
        while index < byte_count {
            let byte_bit_start = (first_byte + index) * 8;
            let mask = byte_cover_mask(start_bit, end_bit, byte_bit_start);
            if mask != 0 {
                let preserve_base = match update {
                    AmlFieldUpdateKind::Preserve => {
                        self.read_region_byte(host, region, first_byte + index)?
                    }
                    AmlFieldUpdateKind::WriteAsOnes => u8::MAX,
                    AmlFieldUpdateKind::WriteAsZeros => 0,
                };
                let payload_bits = ((shifted_value >> (index * 8)) & 0xff) as u8;
                let merged = (preserve_base & !mask) | (payload_bits & mask);
                self.write_region_byte(host, region, first_byte + index, merged)?;
            }
            index += 1;
        }
        Ok(())
    }

    fn eval_dynamic_opregion_statement<'a>(
        &self,
        bytes: &'a [u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)>
    where
        'blocks: 'a,
    {
        let encoded = AmlEncodedNameString::parse(&bytes[2..])?;
        let name = local_single_segment(encoded)?;
        let space_index = 2 + usize::from(encoded.consumed_bytes);
        let space = *bytes.get(space_index).ok_or_else(AmlError::truncated)?;
        let (offset, offset_consumed) =
            self.eval_term_arg(&bytes[space_index + 1..], host, state, phase, frame)?;
        let length_index = space_index + 1 + offset_consumed;
        let (length, length_consumed) =
            self.eval_term_arg(&bytes[length_index..], host, state, phase, frame)?;
        frame.bind_named_region(
            name,
            AmlOpRegionDescriptor {
                node: crate::aml::AmlNamespaceNodeId(u32::MAX),
                space: map_dynamic_address_space(space),
                offset: Some(offset.as_integer()?),
                length: Some(length.as_integer()?),
            },
        )?;
        Ok((length_index + length_consumed, AmlControl::Continue))
    }

    fn eval_dynamic_field_statement<'a>(
        &self,
        bytes: &'a [u8],
        _host: Option<&dyn AmlRegionAccessHost>,
        _state: Option<&AmlRuntimeState<'_>>,
        _phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)>
    where
        'blocks: 'a,
    {
        let pkg = AmlPkgLength::parse(&bytes[2..])?;
        let object_end = 2 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let region_name_offset = 2 + usize::from(pkg.encoded_bytes);
        let region_name = AmlEncodedNameString::parse(&object_bytes[region_name_offset..])?;
        let region = self.resolve_dynamic_region_binding(region_name, frame)?;
        let flags_index = region_name_offset + usize::from(region_name.consumed_bytes);
        let flags = *object_bytes
            .get(flags_index)
            .ok_or_else(AmlError::truncated)?;
        self.bind_dynamic_field_entries(
            &object_bytes[..object_end],
            flags_index + 1,
            object_end,
            region,
            flags,
            frame,
        )?;
        Ok((object_end, AmlControl::Continue))
    }

    fn resolve_dynamic_region_binding(
        &self,
        encoded: AmlEncodedNameString<'_>,
        frame: &AmlEvalFrame<'_>,
    ) -> AmlResult<AmlOpRegionDescriptor> {
        if let Some(name) = local_single_segment_if_present(encoded) {
            if let Some(region) = frame.named_region(name) {
                return Ok(region);
            }
        }
        let path = self
            .namespace
            .resolve_lookup_path(frame.current_scope_path, encoded)?;
        let record = self
            .namespace
            .record_by_path(path)
            .ok_or_else(AmlError::undefined_object)?;
        match record.payload {
            AmlNamespaceNodePayload::OpRegion(region) => Ok(region),
            _ => Err(AmlError::unsupported()),
        }
    }

    fn bind_dynamic_field_entries(
        &self,
        object_bytes: &[u8],
        start_cursor: usize,
        object_end: usize,
        region: AmlOpRegionDescriptor,
        flags: u8,
        frame: &mut AmlEvalFrame<'_>,
    ) -> AmlResult<()> {
        let mut current_bit_offset = 0_u32;
        let mut cursor = start_cursor;
        while cursor < object_end {
            let opcode = object_bytes[cursor];
            match opcode {
                0x00 => {
                    let skip = AmlPkgLength::parse(&object_bytes[cursor + 1..])?;
                    current_bit_offset = current_bit_offset.saturating_add(skip.value);
                    cursor += 1 + usize::from(skip.encoded_bytes);
                }
                0x01 => {
                    cursor += 3;
                }
                0x02 => {
                    let name = AmlEncodedNameString::parse(&object_bytes[cursor + 1..])?;
                    cursor += 1 + usize::from(name.consumed_bytes);
                }
                0x03 => {
                    cursor += 4;
                }
                _ => {
                    let seg = AmlNameSeg::from_bytes([
                        object_bytes[cursor],
                        *object_bytes
                            .get(cursor + 1)
                            .ok_or_else(AmlError::truncated)?,
                        *object_bytes
                            .get(cursor + 2)
                            .ok_or_else(AmlError::truncated)?,
                        *object_bytes
                            .get(cursor + 3)
                            .ok_or_else(AmlError::truncated)?,
                    ])?;
                    let width = AmlPkgLength::parse(&object_bytes[cursor + 4..])?;
                    frame.bind_named_field(
                        seg,
                        AmlDynamicFieldBinding {
                            region,
                            bit_offset: current_bit_offset,
                            bit_width: width.value,
                            access: decode_dynamic_field_access(flags),
                            update: decode_dynamic_field_update(flags),
                        },
                    )?;
                    current_bit_offset = current_bit_offset.saturating_add(width.value);
                    cursor += 4 + usize::from(width.encoded_bytes);
                }
            }
        }
        Ok(())
    }

    fn resolve_region(&self, field: AmlFieldDescriptor) -> AmlResult<AmlOpRegionDescriptor> {
        let region_id = field.region.ok_or_else(AmlError::invalid_state)?;
        let region_record = self
            .namespace
            .record(region_id)
            .ok_or_else(AmlError::undefined_object)?;
        match region_record.payload {
            AmlNamespaceNodePayload::OpRegion(region) => Ok(region),
            _ => Err(AmlError::invalid_state()),
        }
    }

    fn ensure_region_byte_range(
        &self,
        region: AmlOpRegionDescriptor,
        first_byte: u64,
        last_byte: u64,
    ) -> AmlResult<()> {
        if let Some(length) = region.length {
            let required = last_byte.checked_add(1).ok_or_else(AmlError::overflow)?;
            if required > length || first_byte >= length {
                return Err(AmlError::invalid_state());
            }
        }
        Ok(())
    }

    fn read_region_byte(
        &self,
        host: &dyn AmlRegionAccessHost,
        region: AmlOpRegionDescriptor,
        byte_offset: u64,
    ) -> AmlResult<u8> {
        let base = region.offset.ok_or_else(AmlError::unsupported)?;
        match region.space {
            crate::aml::AmlAddressSpaceId::SystemMemory => {
                Ok(host.read_system_memory(base + byte_offset, AmlAccessWidth::Bits8)? as u8)
            }
            crate::aml::AmlAddressSpaceId::SystemIo => {
                Ok(host.read_system_io(base + byte_offset, AmlAccessWidth::Bits8)? as u8)
            }
            crate::aml::AmlAddressSpaceId::PciConfig => {
                Ok(host.read_pci_config(base + byte_offset, AmlAccessWidth::Bits8)? as u8)
            }
            crate::aml::AmlAddressSpaceId::EmbeddedControl => {
                let register =
                    u8::try_from(base + byte_offset).map_err(|_| AmlError::overflow())?;
                host.read_embedded_controller(register)
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn write_region_byte(
        &self,
        host: &dyn AmlRegionAccessHost,
        region: AmlOpRegionDescriptor,
        byte_offset: u64,
        value: u8,
    ) -> AmlResult<()> {
        let base = region.offset.ok_or_else(AmlError::unsupported)?;
        match region.space {
            crate::aml::AmlAddressSpaceId::SystemMemory => host.write_system_memory(
                base + byte_offset,
                AmlAccessWidth::Bits8,
                u64::from(value),
            ),
            crate::aml::AmlAddressSpaceId::SystemIo => {
                host.write_system_io(base + byte_offset, AmlAccessWidth::Bits8, u64::from(value))
            }
            crate::aml::AmlAddressSpaceId::PciConfig => {
                host.write_pci_config(base + byte_offset, AmlAccessWidth::Bits8, u64::from(value))
            }
            crate::aml::AmlAddressSpaceId::EmbeddedControl => {
                let register =
                    u8::try_from(base + byte_offset).map_err(|_| AmlError::overflow())?;
                host.write_embedded_controller(register, value)
            }
            _ => Err(AmlError::unsupported()),
        }
    }
}

fn width_mask(bit_width: u32) -> u64 {
    if bit_width >= 64 {
        u64::MAX
    } else {
        (1_u64 << bit_width) - 1
    }
}

fn byte_cover_mask(start_bit: u64, end_bit: u64, byte_bit_start: u64) -> u8 {
    let byte_bit_end = byte_bit_start + 8;
    let covered_start = start_bit.max(byte_bit_start);
    let covered_end = end_bit.min(byte_bit_end);
    if covered_start >= covered_end {
        return 0;
    }

    let mut mask = 0_u8;
    let mut bit = covered_start - byte_bit_start;
    while bit < covered_end - byte_bit_start {
        mask |= 1_u8 << bit;
        bit += 1;
    }
    mask
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AmlControl<'a> {
    Continue,
    Blocked,
    Break,
    Return(AmlValue<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AmlNamedValueBinding<'a> {
    name: AmlNameSeg,
    binding: AmlNamedBinding<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AmlNamedBinding<'a> {
    Value(AmlValue<'a>),
    OpRegion(AmlOpRegionDescriptor),
    Field(AmlDynamicFieldBinding),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AmlDynamicFieldBinding {
    region: AmlOpRegionDescriptor,
    bit_offset: u32,
    bit_width: u32,
    access: AmlFieldAccessKind,
    update: AmlFieldUpdateKind,
}

struct AmlEvalFrame<'a> {
    integer_width: AmlIntegerWidth,
    current_scope_path: AmlResolvedNamePath,
    args: [Option<AmlValue<'a>>; 7],
    locals: [Option<AmlValue<'a>>; 8],
    named_values: [Option<AmlNamedValueBinding<'a>>; 8],
    recursion_depth: u16,
}

impl<'a> AmlEvalFrame<'a> {
    fn new(
        integer_width: AmlIntegerWidth,
        current_scope_path: AmlResolvedNamePath,
        args: &[AmlValue<'a>],
        recursion_depth: u16,
    ) -> AmlResult<Self> {
        if args.len() > 7 {
            return Err(AmlError::unsupported());
        }
        let mut slots: [Option<AmlValue<'a>>; 7] = array::from_fn(|_| None);
        let mut index = 0_usize;
        while index < args.len() {
            slots[index] = Some(args[index].clone());
            index += 1;
        }
        Ok(Self {
            integer_width,
            current_scope_path,
            args: slots,
            locals: array::from_fn(|_| None),
            named_values: array::from_fn(|_| None),
            recursion_depth,
        })
    }

    fn bind_named_value(&mut self, name: AmlNameSeg, value: AmlValue<'a>) -> AmlResult<()> {
        let mut empty_index = None;
        let mut index = 0_usize;
        while index < self.named_values.len() {
            match &mut self.named_values[index] {
                Some(binding) if binding.name == name => {
                    binding.binding = AmlNamedBinding::Value(value);
                    return Ok(());
                }
                None if empty_index.is_none() => empty_index = Some(index),
                _ => {}
            }
            index += 1;
        }
        let Some(index) = empty_index else {
            return Err(AmlError::overflow());
        };
        self.named_values[index] = Some(AmlNamedValueBinding {
            name,
            binding: AmlNamedBinding::Value(value),
        });
        Ok(())
    }

    fn bind_named_region(
        &mut self,
        name: AmlNameSeg,
        region: AmlOpRegionDescriptor,
    ) -> AmlResult<()> {
        self.bind_named_binding(name, AmlNamedBinding::OpRegion(region))
    }

    fn bind_named_field(
        &mut self,
        name: AmlNameSeg,
        field: AmlDynamicFieldBinding,
    ) -> AmlResult<()> {
        self.bind_named_binding(name, AmlNamedBinding::Field(field))
    }

    fn bind_named_binding(
        &mut self,
        name: AmlNameSeg,
        binding: AmlNamedBinding<'a>,
    ) -> AmlResult<()> {
        let mut empty_index = None;
        let mut index = 0_usize;
        while index < self.named_values.len() {
            match &mut self.named_values[index] {
                Some(existing) if existing.name == name => {
                    existing.binding = binding;
                    return Ok(());
                }
                None if empty_index.is_none() => empty_index = Some(index),
                _ => {}
            }
            index += 1;
        }
        let Some(index) = empty_index else {
            return Err(AmlError::overflow());
        };
        self.named_values[index] = Some(AmlNamedValueBinding { name, binding });
        Ok(())
    }

    fn named_value(&self, name: AmlNameSeg) -> Option<&AmlValue<'a>> {
        self.named_values
            .iter()
            .flatten()
            .find(|binding| binding.name == name)
            .and_then(|binding| match &binding.binding {
                AmlNamedBinding::Value(value) => Some(value),
                _ => None,
            })
    }

    fn named_region(&self, name: AmlNameSeg) -> Option<AmlOpRegionDescriptor> {
        self.named_values
            .iter()
            .flatten()
            .find(|binding| binding.name == name)
            .and_then(|binding| match binding.binding {
                AmlNamedBinding::OpRegion(region) => Some(region),
                _ => None,
            })
    }

    fn named_field(&self, name: AmlNameSeg) -> Option<AmlDynamicFieldBinding> {
        self.named_values
            .iter()
            .flatten()
            .find(|binding| binding.name == name)
            .and_then(|binding| match binding.binding {
                AmlNamedBinding::Field(field) => Some(field),
                _ => None,
            })
    }

    fn write_named_value(&mut self, name: AmlNameSeg, value: AmlValue<'a>) -> bool {
        let mut index = 0_usize;
        while index < self.named_values.len() {
            if let Some(binding) = &mut self.named_values[index] {
                if binding.name == name && matches!(binding.binding, AmlNamedBinding::Value(_)) {
                    binding.binding = AmlNamedBinding::Value(value);
                    return true;
                }
            }
            index += 1;
        }
        false
    }
}

fn local_single_segment(encoded: AmlEncodedNameString<'_>) -> AmlResult<AmlNameSeg> {
    local_single_segment_if_present(encoded).ok_or_else(AmlError::unsupported)
}

fn local_single_segment_if_present(encoded: AmlEncodedNameString<'_>) -> Option<AmlNameSeg> {
    if encoded.is_null
        || encoded.anchor != crate::aml::AmlNameAnchor::Local
        || encoded.parent_prefixes != 0
        || encoded.segment_count != 1
    {
        return None;
    }
    encoded.segment(0)
}

fn map_dynamic_address_space(value: u8) -> AmlAddressSpaceId {
    match value {
        0x00 => AmlAddressSpaceId::SystemMemory,
        0x01 => AmlAddressSpaceId::SystemIo,
        0x02 => AmlAddressSpaceId::PciConfig,
        0x03 => AmlAddressSpaceId::EmbeddedControl,
        0x04 => AmlAddressSpaceId::SmBus,
        0x05 => AmlAddressSpaceId::Cmos,
        0x06 => AmlAddressSpaceId::PciBarTarget,
        0x07 => AmlAddressSpaceId::Ipmi,
        0x08 => AmlAddressSpaceId::Gpio,
        0x09 => AmlAddressSpaceId::GenericSerialBus,
        0x0a => AmlAddressSpaceId::PlatformCommChannel,
        0x7f => AmlAddressSpaceId::FunctionalFixedHardware,
        other => AmlAddressSpaceId::Oem(other),
    }
}

fn decode_dynamic_field_access(flags: u8) -> AmlFieldAccessKind {
    match flags & 0x0f {
        0x00 => AmlFieldAccessKind::Any,
        0x01 => AmlFieldAccessKind::Byte,
        0x02 => AmlFieldAccessKind::Word,
        0x03 => AmlFieldAccessKind::DWord,
        0x04 => AmlFieldAccessKind::QWord,
        0x05 => AmlFieldAccessKind::Buffer,
        _ => AmlFieldAccessKind::Any,
    }
}

fn decode_dynamic_field_update(flags: u8) -> AmlFieldUpdateKind {
    match (flags >> 5) & 0b11 {
        0b01 => AmlFieldUpdateKind::WriteAsOnes,
        0b10 => AmlFieldUpdateKind::WriteAsZeros,
        _ => AmlFieldUpdateKind::Preserve,
    }
}

fn static_package_element_count(bytes: &[u8]) -> AmlResult<u8> {
    let pkg = AmlPkgLength::parse(&bytes[1..])?;
    let object_end = 1 + pkg.value as usize;
    let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
    let count_offset = 1 + usize::from(pkg.encoded_bytes);
    object_bytes
        .get(count_offset)
        .copied()
        .ok_or_else(AmlError::truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aml::{
        AmlAccessWidth,
        AmlDefinitionBlock,
        AmlDefinitionBlockSet,
        AmlEmbeddedControllerHost,
        AmlHost,
        AmlNamespaceLoadRecord,
        AmlNotifyEvent,
        AmlNotifySink,
        AmlOspmInterface,
        AmlPciConfigHost,
        AmlRuntimeIntegerSlot,
        AmlRuntimeMutexSlot,
        AmlRuntimeState,
        AmlSleepHost,
        AmlSystemIoHost,
        AmlSystemMemoryHost,
    };
    use crate::pal::hal::acpi::Dsdt;
    use core::cell::Cell;
    use std::cell::RefCell;
    use std::boxed::Box;
    use std::mem::MaybeUninit;
    use std::vec::Vec;

    fn encode_pkg_length(payload_len: usize) -> Vec<u8> {
        let one_byte_value = payload_len + 1;
        if one_byte_value < 0x40 {
            return vec![one_byte_value as u8];
        }

        let two_byte_value = payload_len + 2;
        vec![
            0b0100_0000 | ((two_byte_value & 0x0f) as u8),
            ((two_byte_value >> 4) & 0xff) as u8,
        ]
    }

    fn pkg(opcode: u8, payload: &[u8]) -> Vec<u8> {
        let pkg_length = encode_pkg_length(payload.len());
        let mut bytes = Vec::with_capacity(1 + pkg_length.len() + payload.len());
        bytes.push(opcode);
        bytes.extend_from_slice(&pkg_length);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn method(name: [u8; 4], flags: u8, body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&name);
        payload.push(flags);
        payload.extend_from_slice(body);
        pkg(0x14, &payload)
    }

    fn scope(name: &[u8], body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(name);
        payload.extend_from_slice(body);
        pkg(0x10, &payload)
    }

    fn opregion(name: [u8; 4], space: u8, offset: u8, length: u8) -> Vec<u8> {
        vec![
            0x5b, 0x80, name[0], name[1], name[2], name[3], space, 0x0a, offset, 0x0a, length,
        ]
    }

    fn mutex(name: [u8; 4], sync_level: u8) -> Vec<u8> {
        vec![0x5b, 0x01, name[0], name[1], name[2], name[3], sync_level]
    }

    fn device(name: [u8; 4], body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&name);
        payload.extend_from_slice(body);
        let pkg_length = encode_pkg_length(payload.len());
        let mut bytes = Vec::with_capacity(2 + pkg_length.len() + payload.len());
        bytes.push(0x5b);
        bytes.push(0x82);
        bytes.extend_from_slice(&pkg_length);
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn field(region: [u8; 4], flags: u8, units: &[([u8; 4], u8)]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&region);
        payload.push(flags);
        for (name, width) in units {
            payload.extend_from_slice(name);
            payload.push(*width);
        }

        let pkg_length = encode_pkg_length(payload.len());
        let mut bytes = Vec::with_capacity(2 + pkg_length.len() + payload.len());
        bytes.push(0x5b);
        bytes.push(0x81);
        bytes.extend_from_slice(&pkg_length);
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn if_else(predicate: &[u8], then_body: &[u8], else_body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(predicate);
        payload.extend_from_slice(then_body);
        let mut bytes = pkg(0xA0, &payload);
        let else_pkg = pkg(0xA1, else_body);
        bytes.extend_from_slice(&else_pkg);
        bytes
    }

    fn while_loop(predicate: &[u8], body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(predicate);
        payload.extend_from_slice(body);
        pkg(0xA2, &payload)
    }

    fn definition_block(payload: &[u8]) -> AmlDefinitionBlock<'static> {
        let mut table = Vec::from([0_u8; 36]);
        table[0..4].copy_from_slice(b"DSDT");
        table[4..8].copy_from_slice(&((36 + payload.len()) as u32).to_le_bytes());
        table[8] = 2;
        table[10..16].copy_from_slice(b"FUSION");
        table[16..24].copy_from_slice(b"AMLEVAL ");
        table.extend_from_slice(payload);
        let checksum =
            (!table.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        table[9] = checksum;
        let leaked = Box::leak(table.into_boxed_slice());
        AmlDefinitionBlock::from_dsdt(Dsdt::parse(leaked).unwrap()).unwrap()
    }

    fn load_namespace(payload: &[u8]) -> AmlLoadedNamespace<'static, 'static> {
        let block = definition_block(payload);
        let plan = crate::aml::AmlNamespaceLoadPlan::from_definition_blocks(
            AmlDefinitionBlockSet::new(block, &[]),
        );
        let storage = Box::leak(Box::new(
            [MaybeUninit::<AmlNamespaceLoadRecord>::uninit(); 32],
        ));
        plan.load_into(storage).unwrap()
    }

    fn root_sb_path() -> AmlResolvedNamePath {
        let mut path = AmlResolvedNamePath::root();
        path.push(crate::aml::AmlNameSeg::from_bytes(*b"_SB_").unwrap())
            .unwrap();
        path
    }

    struct FakeRegionHost {
        ec: RefCell<[u8; 256]>,
        notifications: RefCell<Vec<AmlNotifyEvent>>,
    }

    impl Default for FakeRegionHost {
        fn default() -> Self {
            Self {
                ec: RefCell::new([0; 256]),
                notifications: RefCell::new(Vec::new()),
            }
        }
    }

    impl AmlOspmInterface for FakeRegionHost {
        fn osi_supported(&self, _interface: &str) -> bool {
            false
        }

        fn os_revision(&self) -> u64 {
            0
        }
    }

    impl AmlSleepHost for FakeRegionHost {
        fn stall_us(&self, _microseconds: u32) -> AmlResult<()> {
            Ok(())
        }

        fn sleep_ms(&self, _milliseconds: u32) -> AmlResult<()> {
            Ok(())
        }
    }

    impl AmlNotifySink for FakeRegionHost {
        fn notify(&self, source: crate::aml::AmlNamespaceNodeId, value: u8) -> AmlResult<()> {
            self.notifications
                .borrow_mut()
                .push(AmlNotifyEvent { source, value });
            Ok(())
        }
    }

    impl AmlSystemMemoryHost for FakeRegionHost {
        fn read_system_memory(&self, _address: u64, _width: AmlAccessWidth) -> AmlResult<u64> {
            Err(AmlError::unsupported())
        }

        fn write_system_memory(
            &self,
            _address: u64,
            _width: AmlAccessWidth,
            _value: u64,
        ) -> AmlResult<()> {
            Err(AmlError::unsupported())
        }
    }

    impl AmlSystemIoHost for FakeRegionHost {
        fn read_system_io(&self, _port: u64, _width: AmlAccessWidth) -> AmlResult<u64> {
            Err(AmlError::unsupported())
        }

        fn write_system_io(
            &self,
            _port: u64,
            _width: AmlAccessWidth,
            _value: u64,
        ) -> AmlResult<()> {
            Err(AmlError::unsupported())
        }
    }

    impl AmlPciConfigHost for FakeRegionHost {
        fn read_pci_config(&self, _address: u64, _width: AmlAccessWidth) -> AmlResult<u64> {
            Err(AmlError::unsupported())
        }

        fn write_pci_config(
            &self,
            _address: u64,
            _width: AmlAccessWidth,
            _value: u64,
        ) -> AmlResult<()> {
            Err(AmlError::unsupported())
        }
    }

    impl AmlEmbeddedControllerHost for FakeRegionHost {
        fn read_embedded_controller(&self, register: u8) -> AmlResult<u8> {
            Ok(self.ec.borrow()[usize::from(register)])
        }

        fn write_embedded_controller(&self, register: u8, value: u8) -> AmlResult<()> {
            self.ec.borrow_mut()[usize::from(register)] = value;
            Ok(())
        }
    }

    impl AmlHost for FakeRegionHost {}

    #[test]
    fn pure_evaluator_returns_constant() {
        let body = method(*b"TEST", 0, &[0xA4, 0x0A, 0x2A]);
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);
        let mut path = root_sb_path();
        path.push(crate::aml::AmlNameSeg::from_bytes(*b"TEST").unwrap())
            .unwrap();
        let method_node = namespace.record_by_path(path).unwrap().descriptor.id;
        let evaluator = AmlPureEvaluator::new(namespace);
        let outcome = evaluator
            .evaluate(AmlMethodInvocation {
                method: method_node,
                phase: AmlExecutionPhase::Runtime,
                args: &[],
            })
            .unwrap();
        assert_eq!(outcome.return_value, Some(AmlValue::Integer(42)));
    }

    #[test]
    fn pure_evaluator_adds_args_through_local_store() {
        let body = method(*b"SUM0", 2, &[0x72, 0x68, 0x69, 0x60, 0xA4, 0x60]);
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);
        let mut path = root_sb_path();
        path.push(crate::aml::AmlNameSeg::from_bytes(*b"SUM0").unwrap())
            .unwrap();
        let method_node = namespace.record_by_path(path).unwrap().descriptor.id;
        let evaluator = AmlPureEvaluator::new(namespace);
        let args = [AmlValue::Integer(7), AmlValue::Integer(5)];
        let outcome = evaluator
            .evaluate(AmlMethodInvocation {
                method: method_node,
                phase: AmlExecutionPhase::Runtime,
                args: &args,
            })
            .unwrap();
        assert_eq!(outcome.return_value, Some(AmlValue::Integer(12)));
    }

    #[test]
    fn pure_evaluator_reads_integer_name_and_branches() {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x08, b'F', b'O', b'O', b'0', 0x0A, 0x01]);
        let method_body = if_else(
            &[0x93, 0x68, b'F', b'O', b'O', b'0'],
            &[0xA4, 0x0A, 0x07],
            &[0xA4, 0x0A, 0x09],
        );
        body.extend_from_slice(&method(*b"COND", 1, &method_body));
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);
        let mut path = root_sb_path();
        path.push(crate::aml::AmlNameSeg::from_bytes(*b"COND").unwrap())
            .unwrap();
        let method_node = namespace.record_by_path(path).unwrap().descriptor.id;
        let evaluator = AmlPureEvaluator::new(namespace);

        let true_args = [AmlValue::Integer(1)];
        let false_args = [AmlValue::Integer(0)];

        let true_outcome = evaluator
            .evaluate(AmlMethodInvocation {
                method: method_node,
                phase: AmlExecutionPhase::Runtime,
                args: &true_args,
            })
            .unwrap();
        let false_outcome = evaluator
            .evaluate(AmlMethodInvocation {
                method: method_node,
                phase: AmlExecutionPhase::Runtime,
                args: &false_args,
            })
            .unwrap();

        assert_eq!(true_outcome.return_value, Some(AmlValue::Integer(7)));
        assert_eq!(false_outcome.return_value, Some(AmlValue::Integer(9)));
    }

    #[test]
    fn pure_evaluator_invokes_nested_method() {
        let mut body = Vec::new();
        body.extend_from_slice(&method(
            *b"HLP0",
            1,
            &[0x72, 0x68, 0x0A, 0x02, 0x60, 0xA4, 0x60],
        ));
        body.extend_from_slice(&method(
            *b"CALL",
            0,
            &[0xA4, b'H', b'L', b'P', b'0', 0x0A, 0x05],
        ));
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);
        let mut path = root_sb_path();
        path.push(crate::aml::AmlNameSeg::from_bytes(*b"CALL").unwrap())
            .unwrap();
        let method_node = namespace.record_by_path(path).unwrap().descriptor.id;
        let evaluator = AmlPureEvaluator::new(namespace);
        let outcome = evaluator
            .evaluate(AmlMethodInvocation {
                method: method_node,
                phase: AmlExecutionPhase::Runtime,
                args: &[],
            })
            .unwrap();
        assert_eq!(outcome.return_value, Some(AmlValue::Integer(7)));
    }

    #[test]
    fn evaluator_persists_integer_name_state_across_invocations() {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x08, b'F', b'L', b'G', b'0', 0x0A, 0x00]);
        body.extend_from_slice(&method(*b"SETF", 1, &[0x70, 0x68, b'F', b'L', b'G', b'0']));
        body.extend_from_slice(&method(*b"GETF", 0, &[0xA4, b'F', b'L', b'G', b'0']));
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);

        let mut set_path = root_sb_path();
        set_path
            .push(crate::aml::AmlNameSeg::from_bytes(*b"SETF").unwrap())
            .unwrap();
        let set_node = namespace.record_by_path(set_path).unwrap().descriptor.id;

        let mut get_path = root_sb_path();
        get_path
            .push(crate::aml::AmlNameSeg::from_bytes(*b"GETF").unwrap())
            .unwrap();
        let get_node = namespace.record_by_path(get_path).unwrap().descriptor.id;

        let state_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 4] =
            array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&state_slots);
        let evaluator = AmlPureEvaluator::new(namespace);

        let set_args = [AmlValue::Integer(0x33)];
        evaluator
            .evaluate_with_state(
                &state,
                AmlMethodInvocation {
                    method: set_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &set_args,
                },
            )
            .unwrap();
        let outcome = evaluator
            .evaluate_with_state(
                &state,
                AmlMethodInvocation {
                    method: get_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert_eq!(outcome.return_value, Some(AmlValue::Integer(0x33)));
    }

    #[test]
    fn evaluator_handles_acquire_release_and_unary_updates() {
        let mut body = Vec::new();
        body.extend_from_slice(&mutex(*b"MTX0", 0));
        body.extend_from_slice(&[0x08, b'C', b'N', b'T', b'0', 0x0A, 0x01]);
        body.extend_from_slice(&method(
            *b"BUMP",
            0,
            &[
                0x5b, 0x23, b'M', b'T', b'X', b'0', 0xff, 0xff, // Acquire(MTX0, 0xFFFF)
                0x75, b'C', b'N', b'T', b'0', // Increment(CNT0)
                0x76, b'C', b'N', b'T', b'0', // Decrement(CNT0)
                0x75, b'C', b'N', b'T', b'0', // Increment(CNT0)
                0x70, b'C', b'N', b'T', b'0', 0x60, // Store(CNT0, Local0)
                0x5b, 0x27, b'M', b'T', b'X', b'0', // Release(MTX0)
                0xA4, 0x60, // Return(Local0)
            ],
        ));
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);
        let mut path = root_sb_path();
        path.push(crate::aml::AmlNameSeg::from_bytes(*b"BUMP").unwrap())
            .unwrap();
        let method_node = namespace.record_by_path(path).unwrap().descriptor.id;

        let state_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 4] =
            array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 2] =
            array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&state_slots).with_mutexes(&mutex_slots);
        let evaluator = AmlPureEvaluator::new(namespace);
        let outcome = evaluator
            .evaluate_with_state(
                &state,
                AmlMethodInvocation {
                    method: method_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert_eq!(outcome.return_value, Some(AmlValue::Integer(2)));
    }

    #[test]
    fn evaluator_handles_multiply_divide_while_and_break() {
        let mut body = Vec::new();
        body.extend_from_slice(&[0x08, b'C', b'N', b'T', b'0', 0x0A, 0x00]);
        body.extend_from_slice(&method(
            *b"MATH",
            0,
            &[
                0x77, 0x0A, 0x6B, 0x0A, 0x0A, 0x60, // Multiply(0x6B, 0x0A, Local0)
                0x72, 0x60, 0x0B, 0xAC, 0x0A, 0x60, // Add(Local0, 0x0AAC, Local0)
                0x78, 0x60, 0x0A, 0x0A, 0x61, 0x62, // Divide(Local0, 10, Local1, Local2)
                0x70, 0x00, b'C', b'N', b'T', b'0', // Store(Zero, CNT0)
            ],
        ));
        body.extend_from_slice(&method(*b"LOOP", 0, &{
            let mut bytes = Vec::new();
            let if_break = pkg(0xA0, &[0x93, b'C', b'N', b'T', b'0', 0x0A, 0x03, 0xA5]);
            bytes.extend_from_slice(&while_loop(
                &[0x95, b'C', b'N', b'T', b'0', 0x0A, 0x05], // LLess(CNT0, 5)
                &{
                    let mut body = Vec::new();
                    body.extend_from_slice(&[
                        0x75, b'C', b'N', b'T', b'0', // Increment(CNT0)
                    ]);
                    body.extend_from_slice(&if_break);
                    body
                },
            ));
            bytes.extend_from_slice(&[0xA4, b'C', b'N', b'T', b'0']);
            bytes
        }));
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);
        let evaluator = AmlPureEvaluator::new(namespace);
        let state_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 4] =
            array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 2] =
            array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&state_slots).with_mutexes(&mutex_slots);

        let mut math_path = root_sb_path();
        math_path
            .push(crate::aml::AmlNameSeg::from_bytes(*b"MATH").unwrap())
            .unwrap();
        let math_node = namespace.record_by_path(math_path).unwrap().descriptor.id;
        let math_outcome = evaluator
            .evaluate_with_state(
                &state,
                AmlMethodInvocation {
                    method: math_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert_eq!(math_outcome.return_value, None);

        let mut loop_path = root_sb_path();
        loop_path
            .push(crate::aml::AmlNameSeg::from_bytes(*b"LOOP").unwrap())
            .unwrap();
        let loop_node = namespace.record_by_path(loop_path).unwrap().descriptor.id;
        let loop_outcome = evaluator
            .evaluate_with_state(
                &state,
                AmlMethodInvocation {
                    method: loop_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert_eq!(loop_outcome.return_value, Some(AmlValue::Integer(3)));
    }

    #[test]
    fn evaluator_blocks_when_mutex_is_already_held() {
        let mut body = Vec::new();
        body.extend_from_slice(&mutex(*b"MTX0", 0));
        body.extend_from_slice(&method(
            *b"HOLD",
            0,
            &[0x5b, 0x23, b'M', b'T', b'X', b'0', 0xff, 0xff, 0xA4, 0x01],
        ));
        body.extend_from_slice(&method(
            *b"WAIT",
            0,
            &[0x5b, 0x23, b'M', b'T', b'X', b'0', 0xff, 0xff, 0xA4, 0x01],
        ));
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);

        let mut hold_path = root_sb_path();
        hold_path
            .push(crate::aml::AmlNameSeg::from_bytes(*b"HOLD").unwrap())
            .unwrap();
        let hold_node = namespace.record_by_path(hold_path).unwrap().descriptor.id;

        let mut wait_path = root_sb_path();
        wait_path
            .push(crate::aml::AmlNameSeg::from_bytes(*b"WAIT").unwrap())
            .unwrap();
        let wait_node = namespace.record_by_path(wait_path).unwrap().descriptor.id;

        let state_slots: [Cell<Option<AmlRuntimeIntegerSlot>>; 1] =
            array::from_fn(|_| Cell::new(None));
        let mutex_slots: [Cell<Option<AmlRuntimeMutexSlot>>; 2] =
            array::from_fn(|_| Cell::new(None));
        let state = AmlRuntimeState::new(&state_slots).with_mutexes(&mutex_slots);
        let evaluator = AmlPureEvaluator::new(namespace);

        let hold_outcome = evaluator
            .evaluate_with_state(
                &state,
                AmlMethodInvocation {
                    method: hold_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert!(!hold_outcome.blocked);

        let wait_outcome = evaluator
            .evaluate_with_state(
                &state,
                AmlMethodInvocation {
                    method: wait_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert!(wait_outcome.blocked);
        assert_eq!(wait_outcome.return_value, None);
    }

    #[test]
    fn evaluator_routes_notify_through_host_sink() {
        let mut body = Vec::new();
        body.extend_from_slice(&device(*b"DEV0", &[]));
        body.extend_from_slice(&method(
            *b"NTFY",
            0,
            &[0x5b, 0x86, b'D', b'E', b'V', b'0', 0x0a, 0x80],
        ));
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);
        let mut path = root_sb_path();
        path.push(crate::aml::AmlNameSeg::from_bytes(*b"NTFY").unwrap())
            .unwrap();
        let method_node = namespace.record_by_path(path).unwrap().descriptor.id;
        let dev_path = {
            let mut path = root_sb_path();
            path.push(crate::aml::AmlNameSeg::from_bytes(*b"DEV0").unwrap())
                .unwrap();
            path
        };
        let dev_node = namespace.record_by_path(dev_path).unwrap().descriptor.id;

        let host = FakeRegionHost::default();
        let evaluator = AmlPureEvaluator::new(namespace);
        let outcome = evaluator
            .evaluate_with_host(
                &host,
                AmlMethodInvocation {
                    method: method_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert!(!outcome.blocked);
        let notifications = host.notifications.borrow();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].source, dev_node);
        assert_eq!(notifications[0].value, 0x80);
    }

    #[test]
    fn evaluator_reads_and_writes_embedded_controller_fields() {
        let mut body = Vec::new();
        body.extend_from_slice(&opregion(*b"ECRG", 0x03, 0x10, 0x10));
        body.extend_from_slice(&field(*b"ECRG", 0x01, &[(*b"ST00", 8), (*b"ST01", 8)]));
        body.extend_from_slice(&method(*b"GETH", 0, &[0xA4, b'S', b'T', b'0', b'0']));
        body.extend_from_slice(&method(*b"GET0", 0, &[0xA4, b'G', b'E', b'T', b'H']));
        body.extend_from_slice(&method(
            *b"SET1",
            1,
            &[
                0x70, 0x68, b'S', b'T', b'0', b'1', 0xA4, b'S', b'T', b'0', b'1',
            ],
        ));
        let payload = scope(b"\\_SB_", &body);
        let namespace = load_namespace(&payload);

        let mut get_path = root_sb_path();
        get_path
            .push(crate::aml::AmlNameSeg::from_bytes(*b"GET0").unwrap())
            .unwrap();
        let get_node = namespace.record_by_path(get_path).unwrap().descriptor.id;

        let mut set_path = root_sb_path();
        set_path
            .push(crate::aml::AmlNameSeg::from_bytes(*b"SET1").unwrap())
            .unwrap();
        let set_node = namespace.record_by_path(set_path).unwrap().descriptor.id;

        let host = FakeRegionHost::default();
        host.ec.borrow_mut()[0x10] = 0x2a;

        let evaluator = AmlPureEvaluator::new(namespace);
        let read_outcome = evaluator
            .evaluate_with_host(
                &host,
                AmlMethodInvocation {
                    method: get_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &[],
                },
            )
            .unwrap();
        assert_eq!(read_outcome.return_value, Some(AmlValue::Integer(0x2a)));

        let args = [AmlValue::Integer(0x55)];
        let write_outcome = evaluator
            .evaluate_with_host(
                &host,
                AmlMethodInvocation {
                    method: set_node,
                    phase: AmlExecutionPhase::Runtime,
                    args: &args,
                },
            )
            .unwrap();
        assert_eq!(write_outcome.return_value, Some(AmlValue::Integer(0x55)));
        assert_eq!(host.ec.borrow()[0x11], 0x55);
    }
}
