//! AML method evaluation for the current proving subset.

use core::array;

use crate::aml::{
    AmlAccessWidth,
    AmlEncodedNameString,
    AmlError,
    AmlFieldDescriptor,
    AmlFieldUpdateKind,
    AmlExecutionPhase,
    AmlLoadedNamespace,
    AmlIntegerWidth,
    AmlMethodDescriptor,
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
    ) -> AmlResult<AmlEvaluationOutcome<'a>> {
        self.evaluate_internal(None, None, invocation)
    }

    pub fn evaluate_with_state<'a>(
        &self,
        state: &AmlRuntimeState<'_>,
        invocation: AmlMethodInvocation<'a>,
    ) -> AmlResult<AmlEvaluationOutcome<'a>> {
        self.evaluate_internal(None, Some(state), invocation)
    }

    pub fn evaluate_with_host<'a>(
        &self,
        host: &dyn AmlRegionAccessHost,
        invocation: AmlMethodInvocation<'a>,
    ) -> AmlResult<AmlEvaluationOutcome<'a>> {
        self.evaluate_internal(Some(host), None, invocation)
    }

    pub fn evaluate_with_host_and_state<'a>(
        &self,
        host: &dyn AmlRegionAccessHost,
        state: &AmlRuntimeState<'_>,
        invocation: AmlMethodInvocation<'a>,
    ) -> AmlResult<AmlEvaluationOutcome<'a>> {
        self.evaluate_internal(Some(host), Some(state), invocation)
    }

    fn evaluate_internal<'a>(
        &self,
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        invocation: AmlMethodInvocation<'a>,
    ) -> AmlResult<AmlEvaluationOutcome<'a>> {
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
            AmlControl::Return(value) => Ok(AmlEvaluationOutcome {
                return_value: Some(value),
                blocked: false,
            }),
        }
    }

    fn eval_term_list<'a>(
        &self,
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<AmlControl<'a>> {
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
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)> {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x70 => {
                let (value, value_consumed) =
                    self.eval_term_arg(&bytes[1..], host, state, phase, frame)?;
                let target_consumed = self.assign_target(
                    &bytes[1 + value_consumed..],
                    host,
                    state,
                    frame,
                    value.clone(),
                )?;
                Ok((1 + value_consumed + target_consumed, AmlControl::Continue))
            }
            0x5b => self.eval_ext_statement(bytes, host, state, phase, frame),
            0xA0 => self.eval_if(bytes, host, state, phase, frame),
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
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)> {
        let sub = *bytes.get(1).ok_or_else(AmlError::truncated)?;
        match sub {
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
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(usize, AmlControl<'a>)> {
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

    fn eval_term_arg<'a>(
        &self,
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)> {
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
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        frame: &mut AmlEvalFrame<'a>,
        op: F,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
        F: FnOnce(u64, AmlIntegerWidth) -> AmlValue<'a>,
    {
        let (value, target_consumed) = self.read_target_value(&bytes[1..], host, state, frame)?;
        let result = op(value.as_integer()?, frame.integer_width);
        self.assign_target(&bytes[1..], host, state, frame, result.clone())?;
        Ok((result, 1 + target_consumed))
    }

    fn eval_named_term<'a>(
        &self,
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)> {
        let encoded = AmlEncodedNameString::parse(bytes)?;
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
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
        op: F,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
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
            frame,
            result.clone(),
        )?;
        Ok((result, 1 + lhs_consumed + rhs_consumed + target_consumed))
    }

    fn eval_logic_compare<'a, F>(
        &self,
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
        predicate: F,
    ) -> AmlResult<(AmlValue<'a>, usize)>
    where
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

    fn invoke_method_from_term<'a>(
        &self,
        method: AmlMethodDescriptor,
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        phase: AmlExecutionPhase,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)> {
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
    ) -> AmlResult<Option<AmlValue<'a>>> {
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
            AmlControl::Return(value) => Ok(Some(value.clone())),
        }
    }

    fn assign_target<'a>(
        &self,
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        frame: &mut AmlEvalFrame<'a>,
        value: AmlValue<'a>,
    ) -> AmlResult<usize> {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x00 => Ok(1),
            0x60..=0x67 => {
                frame.locals[usize::from(opcode - 0x60)] = Some(value);
                Ok(1)
            }
            0x68..=0x6e => Err(AmlError::unsupported()),
            b'\\' | b'^' | b'_' | b'A'..=b'Z' => {
                let encoded = AmlEncodedNameString::parse(bytes)?;
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
        bytes: &[u8],
        host: Option<&dyn AmlRegionAccessHost>,
        state: Option<&AmlRuntimeState<'_>>,
        frame: &mut AmlEvalFrame<'a>,
    ) -> AmlResult<(AmlValue<'a>, usize)> {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x60..=0x67 => {
                let value = frame.locals[usize::from(opcode - 0x60)]
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

    fn read_field_value(
        &self,
        host: &dyn AmlRegionAccessHost,
        field: AmlFieldDescriptor,
    ) -> AmlResult<u64> {
        let region = self.resolve_region(field)?;
        if field.bit_width == 0 || field.bit_width > 64 {
            return Err(AmlError::unsupported());
        }
        let start_bit = u64::from(field.bit_offset);
        let end_bit = start_bit
            .checked_add(u64::from(field.bit_width))
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
        Ok((aggregate >> shift) & width_mask(field.bit_width))
    }

    fn write_field_value(
        &self,
        host: &dyn AmlRegionAccessHost,
        field: AmlFieldDescriptor,
        value: u64,
    ) -> AmlResult<()> {
        let region = self.resolve_region(field)?;
        if field.bit_width == 0 || field.bit_width > 64 {
            return Err(AmlError::unsupported());
        }
        let start_bit = u64::from(field.bit_offset);
        let end_bit = start_bit
            .checked_add(u64::from(field.bit_width))
            .ok_or_else(AmlError::overflow)?;
        let first_byte = start_bit / 8;
        let last_byte = end_bit.saturating_sub(1) / 8;
        self.ensure_region_byte_range(region, first_byte, last_byte)?;

        let masked_value = value & width_mask(field.bit_width);
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
                let preserve_base = match field.update {
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
    Return(AmlValue<'a>),
}

struct AmlEvalFrame<'a> {
    integer_width: AmlIntegerWidth,
    current_scope_path: AmlResolvedNamePath,
    args: [Option<AmlValue<'a>>; 7],
    locals: [Option<AmlValue<'a>>; 8],
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
            recursion_depth,
        })
    }
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
