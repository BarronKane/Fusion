//! AML definition-block loading and namespace plan vocabulary.

use core::mem::MaybeUninit;
use core::slice;

use crate::aml::{
    AmlAddressSpaceId,
    AmlBytecodeSpan,
    AmlCodeLocation,
    AmlDefinitionBlock,
    AmlEncodedNameString,
    AmlError,
    AmlFieldAccessKind,
    AmlFieldDescriptor,
    AmlFieldUpdateKind,
    AmlIntegerWidth,
    AmlLoadedNamespace,
    AmlMethodDescriptor,
    AmlMethodKind,
    AmlMethodSerialization,
    AmlNameSeg,
    AmlNamespace,
    AmlNamespaceLoadRecord,
    AmlNamespaceNodeDescriptor,
    AmlNamespaceNodeId,
    AmlNamespaceNodePayload,
    AmlNamespaceState,
    AmlObjectKind,
    AmlOpRegionDescriptor,
    AmlPkgLength,
    AmlResolvedNamePath,
    AmlResult,
};

/// Borrowed definition-block set for one AML namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlDefinitionBlockSet<'a> {
    pub dsdt: AmlDefinitionBlock<'a>,
    pub ssdts: &'a [AmlDefinitionBlock<'a>],
}

/// Namespace loading phase ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmlLoadPhase {
    DefinitionBlocks,
    RegionRegistration,
    DeviceInitialization,
}

/// Coarse AML load plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmlNamespaceLoadPlan<'a> {
    pub blocks: AmlDefinitionBlockSet<'a>,
    pub integer_width: AmlIntegerWidth,
}

impl<'a> AmlDefinitionBlockSet<'a> {
    #[must_use]
    pub const fn new(dsdt: AmlDefinitionBlock<'a>, ssdts: &'a [AmlDefinitionBlock<'a>]) -> Self {
        Self { dsdt, ssdts }
    }

    #[must_use]
    pub const fn total_block_count(self) -> usize {
        1 + self.ssdts.len()
    }

    #[must_use]
    pub fn block(self, index: u16) -> Option<AmlDefinitionBlock<'a>> {
        match index {
            0 => Some(self.dsdt),
            other => self.ssdts.get(usize::from(other - 1)).copied(),
        }
    }
}

impl<'a> AmlNamespaceLoadPlan<'a> {
    #[must_use]
    pub const fn from_definition_blocks(blocks: AmlDefinitionBlockSet<'a>) -> Self {
        Self {
            integer_width: AmlIntegerWidth::from_definition_block_revision(
                blocks.dsdt.header.revision,
            ),
            blocks,
        }
    }

    #[must_use]
    pub const fn initial_namespace(self) -> AmlNamespace {
        AmlNamespace {
            state: AmlNamespaceState::DefinitionBlocksLoaded,
            node_count: 1,
        }
    }

    #[must_use]
    pub const fn phase_order() -> [AmlLoadPhase; 3] {
        [
            AmlLoadPhase::DefinitionBlocks,
            AmlLoadPhase::RegionRegistration,
            AmlLoadPhase::DeviceInitialization,
        ]
    }

    pub fn load_into<'storage>(
        self,
        storage: &'storage mut [MaybeUninit<AmlNamespaceLoadRecord>],
    ) -> AmlResult<AmlLoadedNamespace<'storage, 'a>> {
        AmlNamespaceLoader::new(self, storage)?.load()
    }
}

struct AmlNamespaceLoader<'plan, 'storage> {
    plan: AmlNamespaceLoadPlan<'plan>,
    storage: &'storage mut [MaybeUninit<AmlNamespaceLoadRecord>],
    len: usize,
    next_id: u32,
}

impl<'plan, 'storage> AmlNamespaceLoader<'plan, 'storage> {
    const ROOT_NODE_ID: AmlNamespaceNodeId = AmlNamespaceNodeId(0);

    fn new(
        plan: AmlNamespaceLoadPlan<'plan>,
        storage: &'storage mut [MaybeUninit<AmlNamespaceLoadRecord>],
    ) -> AmlResult<Self> {
        let mut loader = Self {
            plan,
            storage,
            len: 0,
            next_id: 0,
        };
        loader.push_record(
            AmlNamespaceNodeDescriptor {
                id: Self::ROOT_NODE_ID,
                parent: None,
                kind: AmlObjectKind::Scope,
                path: AmlResolvedNamePath::root(),
            },
            None,
            AmlNamespaceNodePayload::None,
        )?;
        Ok(loader)
    }

    fn load(mut self) -> AmlResult<AmlLoadedNamespace<'storage, 'plan>> {
        let root_path = AmlResolvedNamePath::root();
        self.walk_term_list(
            self.plan.blocks.dsdt.bytes,
            0,
            0,
            root_path,
            Self::ROOT_NODE_ID,
        )?;

        for (index, block) in self.plan.blocks.ssdts.iter().enumerate() {
            self.walk_term_list(
                block.bytes,
                0,
                u16::try_from(index + 1).map_err(|_| AmlError::overflow())?,
                root_path,
                Self::ROOT_NODE_ID,
            )?;
        }

        let records = unsafe {
            slice::from_raw_parts(
                self.storage.as_ptr().cast::<AmlNamespaceLoadRecord>(),
                self.len,
            )
        };

        Ok(AmlLoadedNamespace {
            namespace: AmlNamespace {
                state: AmlNamespaceState::DefinitionBlocksLoaded,
                node_count: self.len as u32,
            },
            records,
            blocks: self.plan.blocks,
        })
    }

    fn walk_term_list(
        &mut self,
        bytes: &[u8],
        base_offset: u32,
        block_index: u16,
        current_scope_path: AmlResolvedNamePath,
        current_scope_id: AmlNamespaceNodeId,
    ) -> AmlResult<()> {
        let mut offset = 0_usize;
        while offset < bytes.len() {
            let consumed = self.parse_term_object(
                &bytes[offset..],
                base_offset + offset as u32,
                block_index,
                current_scope_path,
                current_scope_id,
            )?;
            if consumed == 0 {
                return Err(AmlError::invalid_bytecode());
            }
            offset += consumed;
        }
        Ok(())
    }

    fn parse_term_object(
        &mut self,
        bytes: &[u8],
        absolute_offset: u32,
        block_index: u16,
        current_scope_path: AmlResolvedNamePath,
        current_scope_id: AmlNamespaceNodeId,
    ) -> AmlResult<usize> {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x06 => self.parse_alias_op(bytes, current_scope_path),
            0x08 => self.parse_name_op(bytes, absolute_offset, block_index, current_scope_path),
            0x10 => self.parse_scope_op(bytes, absolute_offset, block_index, current_scope_path),
            0x14 => self.parse_method_op(bytes, absolute_offset, block_index, current_scope_path),
            0x15 => self.parse_external_op(bytes, current_scope_path),
            0xa0 => self.parse_if_op(
                bytes,
                absolute_offset,
                block_index,
                current_scope_path,
                current_scope_id,
            ),
            0x8a..=0x8d | 0x8f => self.parse_create_field_like_op(bytes, current_scope_path, 2),
            0x5b => self.parse_ext_op(
                bytes,
                absolute_offset,
                block_index,
                current_scope_path,
                current_scope_id,
            ),
            _ => Err(AmlError::new(
                crate::aml::AmlErrorKind::Unsupported,
                "unsupported aml namespace term object",
            )),
        }
    }

    fn parse_alias_op(
        &mut self,
        bytes: &[u8],
        current_scope_path: AmlResolvedNamePath,
    ) -> AmlResult<usize> {
        let source = AmlEncodedNameString::parse(&bytes[1..])?;
        let target_offset = 1 + usize::from(source.consumed_bytes);
        let target = AmlEncodedNameString::parse(&bytes[target_offset..])?;
        let target_path = current_scope_path.resolve(target)?;
        let parent_id = self.ensure_scope_path(target_path.parent())?;
        self.insert_unique_record(
            target_path,
            parent_id,
            AmlObjectKind::Alias,
            None,
            AmlNamespaceNodePayload::None,
        )?;
        Ok(1 + usize::from(source.consumed_bytes) + usize::from(target.consumed_bytes))
    }

    fn parse_name_op(
        &mut self,
        bytes: &[u8],
        absolute_offset: u32,
        block_index: u16,
        current_scope_path: AmlResolvedNamePath,
    ) -> AmlResult<usize> {
        let name = AmlEncodedNameString::parse(&bytes[1..])?;
        let path = current_scope_path.resolve(name)?;
        let parent_id = self.ensure_scope_path(path.parent())?;
        let data_offset = 1 + usize::from(name.consumed_bytes);
        let (name_value, data_consumed) = parse_name_initializer(&bytes[data_offset..])?;
        self.insert_unique_record(
            path,
            parent_id,
            AmlObjectKind::Name,
            Some(AmlCodeLocation {
                block_index,
                span: AmlBytecodeSpan {
                    offset: absolute_offset + data_offset as u32,
                    length: data_consumed as u32,
                },
            }),
            match name_value {
                Some(value) => AmlNamespaceNodePayload::NameInteger(value),
                None => AmlNamespaceNodePayload::None,
            },
        )?;
        Ok(data_offset + data_consumed)
    }

    fn parse_if_op(
        &mut self,
        bytes: &[u8],
        absolute_offset: u32,
        block_index: u16,
        current_scope_path: AmlResolvedNamePath,
        current_scope_id: AmlNamespaceNodeId,
    ) -> AmlResult<usize> {
        let pkg = AmlPkgLength::parse(&bytes[1..])?;
        let object_end = 1 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let predicate_offset = 1 + usize::from(pkg.encoded_bytes);
        let (predicate, predicate_consumed) = self
            .evaluate_load_time_term_arg(&object_bytes[predicate_offset..], current_scope_path)?;
        let body_start = predicate_offset + predicate_consumed;
        if predicate != 0 {
            self.walk_term_list(
                &object_bytes[body_start..],
                absolute_offset + body_start as u32,
                block_index,
                current_scope_path,
                current_scope_id,
            )?;
        }

        let mut consumed = object_end;
        if bytes.get(object_end) == Some(&0xa1) {
            let else_pkg = AmlPkgLength::parse(&bytes[object_end + 1..])?;
            let else_end = object_end + 1 + else_pkg.value as usize;
            let else_bytes = bytes
                .get(object_end..else_end)
                .ok_or_else(AmlError::truncated)?;
            let else_body_start = 1 + usize::from(else_pkg.encoded_bytes);
            if predicate == 0 {
                self.walk_term_list(
                    &else_bytes[else_body_start..],
                    absolute_offset + object_end as u32 + else_body_start as u32,
                    block_index,
                    current_scope_path,
                    current_scope_id,
                )?;
            }
            consumed = else_end;
        }

        Ok(consumed)
    }

    fn parse_external_op(
        &mut self,
        bytes: &[u8],
        current_scope_path: AmlResolvedNamePath,
    ) -> AmlResult<usize> {
        let name = AmlEncodedNameString::parse(&bytes[1..])?;
        let path = current_scope_path.resolve(name)?;
        let parent_id = self.ensure_scope_path(path.parent())?;
        if self.find_record(path).is_none() {
            self.insert_unique_record(
                path,
                parent_id,
                AmlObjectKind::External,
                None,
                AmlNamespaceNodePayload::None,
            )?;
        }
        let object_type_index = 1 + usize::from(name.consumed_bytes);
        let _object_type = *bytes
            .get(object_type_index)
            .ok_or_else(AmlError::truncated)?;
        let _arg_count = *bytes
            .get(object_type_index + 1)
            .ok_or_else(AmlError::truncated)?;
        Ok(object_type_index + 2)
    }

    fn parse_create_field_like_op(
        &mut self,
        bytes: &[u8],
        current_scope_path: AmlResolvedNamePath,
        term_arg_count: u8,
    ) -> AmlResult<usize> {
        let mut cursor = if bytes.first() == Some(&0x5b) { 2 } else { 1 };
        let mut remaining = term_arg_count;
        while remaining != 0 {
            let (_, consumed) = parse_term_arg(&bytes[cursor..])?;
            cursor += consumed;
            remaining -= 1;
        }
        let name = AmlEncodedNameString::parse(&bytes[cursor..])?;
        let path = current_scope_path.resolve(name)?;
        let parent_id = self.ensure_scope_path(path.parent())?;
        self.insert_unique_record(
            path,
            parent_id,
            AmlObjectKind::BufferField,
            None,
            AmlNamespaceNodePayload::None,
        )?;
        Ok(cursor + usize::from(name.consumed_bytes))
    }

    fn parse_scope_op(
        &mut self,
        bytes: &[u8],
        absolute_offset: u32,
        block_index: u16,
        current_scope_path: AmlResolvedNamePath,
    ) -> AmlResult<usize> {
        let pkg = AmlPkgLength::parse(&bytes[1..])?;
        let object_end = 1 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let name_offset = 1 + usize::from(pkg.encoded_bytes);
        let name = AmlEncodedNameString::parse(&object_bytes[name_offset..])?;
        let path = current_scope_path.resolve(name)?;
        let scope_id = self.ensure_scope_like_path(path)?;
        let body_start = name_offset + usize::from(name.consumed_bytes);
        self.walk_term_list(
            &object_bytes[body_start..],
            absolute_offset + body_start as u32,
            block_index,
            path,
            scope_id,
        )?;
        Ok(object_end)
    }

    fn parse_method_op(
        &mut self,
        bytes: &[u8],
        absolute_offset: u32,
        block_index: u16,
        current_scope_path: AmlResolvedNamePath,
    ) -> AmlResult<usize> {
        let pkg = AmlPkgLength::parse(&bytes[1..])?;
        let object_end = 1 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let name_offset = 1 + usize::from(pkg.encoded_bytes);
        let name = AmlEncodedNameString::parse(&object_bytes[name_offset..])?;
        let path = current_scope_path.resolve(name)?;
        let parent_id = self.ensure_scope_path(path.parent())?;
        let flags_index = name_offset + usize::from(name.consumed_bytes);
        let flags = *object_bytes
            .get(flags_index)
            .ok_or_else(AmlError::truncated)?;
        let body_start = flags_index + 1;
        let method_id = self.next_node_id();
        let descriptor = AmlMethodDescriptor {
            node: method_id,
            arg_count: flags & 0b111,
            serialization: if flags & 0b1000 != 0 {
                AmlMethodSerialization::Serialized
            } else {
                AmlMethodSerialization::NotSerialized
            },
            sync_level: flags >> 4,
            kind: classify_method_kind(path),
            body: AmlCodeLocation {
                block_index,
                span: AmlBytecodeSpan {
                    offset: absolute_offset + body_start as u32,
                    length: (object_end - body_start) as u32,
                },
            },
        };
        self.insert_record_with_id(
            AmlNamespaceNodeDescriptor {
                id: method_id,
                parent: parent_id,
                kind: AmlObjectKind::Method,
                path,
            },
            Some(descriptor.body),
            AmlNamespaceNodePayload::Method(descriptor),
        )?;
        Ok(object_end)
    }

    fn parse_ext_op(
        &mut self,
        bytes: &[u8],
        absolute_offset: u32,
        block_index: u16,
        current_scope_path: AmlResolvedNamePath,
        current_scope_id: AmlNamespaceNodeId,
    ) -> AmlResult<usize> {
        let sub = *bytes.get(1).ok_or_else(AmlError::truncated)?;
        match sub {
            0x01 => self.parse_mutex_op(bytes, current_scope_path),
            0x02 => self.parse_event_op(bytes, current_scope_path),
            0x13 => self.parse_create_field_like_op(bytes, current_scope_path, 3),
            0x80 => self.parse_opregion_op(bytes, current_scope_path),
            0x81 => {
                self.parse_field_op(bytes, absolute_offset, current_scope_path, current_scope_id)
            }
            0x86 => self.parse_index_field_op(bytes, current_scope_path, current_scope_id),
            0x82 => self.parse_pkg_scoped_named_object(
                bytes,
                absolute_offset,
                block_index,
                current_scope_path,
                AmlObjectKind::Device,
                0,
            ),
            0x83 => self.parse_pkg_scoped_named_object(
                bytes,
                absolute_offset,
                block_index,
                current_scope_path,
                AmlObjectKind::Processor,
                6,
            ),
            0x84 => self.parse_pkg_scoped_named_object(
                bytes,
                absolute_offset,
                block_index,
                current_scope_path,
                AmlObjectKind::PowerResource,
                3,
            ),
            0x85 => self.parse_pkg_scoped_named_object(
                bytes,
                absolute_offset,
                block_index,
                current_scope_path,
                AmlObjectKind::ThermalZone,
                0,
            ),
            _ => Err(AmlError::new(
                crate::aml::AmlErrorKind::Unsupported,
                "unsupported aml namespace ext op",
            )),
        }
    }

    fn parse_mutex_op(
        &mut self,
        bytes: &[u8],
        current_scope_path: AmlResolvedNamePath,
    ) -> AmlResult<usize> {
        let name = AmlEncodedNameString::parse(&bytes[2..])?;
        let path = current_scope_path.resolve(name)?;
        let parent_id = self.ensure_scope_path(path.parent())?;
        let sync_index = 2 + usize::from(name.consumed_bytes);
        let _sync_level = *bytes.get(sync_index).ok_or_else(AmlError::truncated)?;
        self.insert_unique_record(
            path,
            parent_id,
            AmlObjectKind::Mutex,
            None,
            AmlNamespaceNodePayload::None,
        )?;
        Ok(sync_index + 1)
    }

    fn parse_event_op(
        &mut self,
        bytes: &[u8],
        current_scope_path: AmlResolvedNamePath,
    ) -> AmlResult<usize> {
        let name = AmlEncodedNameString::parse(&bytes[2..])?;
        let path = current_scope_path.resolve(name)?;
        let parent_id = self.ensure_scope_path(path.parent())?;
        self.insert_unique_record(
            path,
            parent_id,
            AmlObjectKind::Event,
            None,
            AmlNamespaceNodePayload::None,
        )?;
        Ok(2 + usize::from(name.consumed_bytes))
    }

    fn parse_opregion_op(
        &mut self,
        bytes: &[u8],
        current_scope_path: AmlResolvedNamePath,
    ) -> AmlResult<usize> {
        let name = AmlEncodedNameString::parse(&bytes[2..])?;
        let path = current_scope_path.resolve(name)?;
        let parent_id = self.ensure_scope_path(path.parent())?;
        let space_index = 2 + usize::from(name.consumed_bytes);
        let space = *bytes.get(space_index).ok_or_else(AmlError::truncated)?;
        let (offset_value, offset_consumed) = parse_term_arg(&bytes[space_index + 1..])?;
        let length_index = space_index + 1 + offset_consumed;
        let (length_value, length_consumed) = parse_term_arg(&bytes[length_index..])?;

        let node_id = self.next_node_id();
        let payload = AmlNamespaceNodePayload::OpRegion(AmlOpRegionDescriptor {
            node: node_id,
            space: map_address_space(space),
            offset: offset_value,
            length: length_value,
        });
        self.insert_record_with_id(
            AmlNamespaceNodeDescriptor {
                id: node_id,
                parent: parent_id,
                kind: AmlObjectKind::OpRegion,
                path,
            },
            None,
            payload,
        )?;
        Ok(length_index + length_consumed)
    }

    fn parse_field_op(
        &mut self,
        bytes: &[u8],
        _absolute_offset: u32,
        current_scope_path: AmlResolvedNamePath,
        current_scope_id: AmlNamespaceNodeId,
    ) -> AmlResult<usize> {
        let pkg = AmlPkgLength::parse(&bytes[2..])?;
        let object_end = 2 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let region_name_offset = 2 + usize::from(pkg.encoded_bytes);
        let region_name = AmlEncodedNameString::parse(&object_bytes[region_name_offset..])?;
        let region_path = current_scope_path.resolve(region_name)?;
        let region_id = self.find_node_id(region_path);
        let flags_index = region_name_offset + usize::from(region_name.consumed_bytes);
        let flags = *object_bytes
            .get(flags_index)
            .ok_or_else(AmlError::truncated)?;
        self.parse_field_entries(
            &object_bytes[..object_end],
            flags_index + 1,
            object_end,
            current_scope_path,
            current_scope_id,
            region_id,
            flags,
        )?;
        Ok(object_end)
    }

    fn parse_index_field_op(
        &mut self,
        bytes: &[u8],
        current_scope_path: AmlResolvedNamePath,
        current_scope_id: AmlNamespaceNodeId,
    ) -> AmlResult<usize> {
        let pkg = AmlPkgLength::parse(&bytes[2..])?;
        let object_end = 2 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let index_name_offset = 2 + usize::from(pkg.encoded_bytes);
        let index_name = AmlEncodedNameString::parse(&object_bytes[index_name_offset..])?;
        let data_name_offset = index_name_offset + usize::from(index_name.consumed_bytes);
        let data_name = AmlEncodedNameString::parse(&object_bytes[data_name_offset..])?;
        let flags_index = data_name_offset + usize::from(data_name.consumed_bytes);
        let flags = *object_bytes
            .get(flags_index)
            .ok_or_else(AmlError::truncated)?;
        self.parse_field_entries(
            &object_bytes[..object_end],
            flags_index + 1,
            object_end,
            current_scope_path,
            current_scope_id,
            None,
            flags,
        )?;
        Ok(object_end)
    }

    fn parse_pkg_scoped_named_object(
        &mut self,
        bytes: &[u8],
        absolute_offset: u32,
        block_index: u16,
        current_scope_path: AmlResolvedNamePath,
        kind: AmlObjectKind,
        fixed_prefix_bytes: usize,
    ) -> AmlResult<usize> {
        let pkg = AmlPkgLength::parse(&bytes[2..])?;
        let object_end = 2 + pkg.value as usize;
        let object_bytes = bytes.get(..object_end).ok_or_else(AmlError::truncated)?;
        let name_offset = 2 + usize::from(pkg.encoded_bytes);
        let name = AmlEncodedNameString::parse(&object_bytes[name_offset..])?;
        let path = current_scope_path.resolve(name)?;
        let parent_id = self.ensure_scope_path(path.parent())?;
        let node_id = self.insert_unique_record(
            path,
            parent_id,
            kind,
            Some(AmlCodeLocation {
                block_index,
                span: AmlBytecodeSpan {
                    offset: absolute_offset
                        + (name_offset + usize::from(name.consumed_bytes) + fixed_prefix_bytes)
                            as u32,
                    length: (object_end
                        - (name_offset + usize::from(name.consumed_bytes) + fixed_prefix_bytes))
                        as u32,
                },
            }),
            AmlNamespaceNodePayload::None,
        )?;
        let body_start = name_offset + usize::from(name.consumed_bytes) + fixed_prefix_bytes;
        self.walk_term_list(
            &object_bytes[body_start..],
            absolute_offset + body_start as u32,
            block_index,
            path,
            node_id,
        )?;
        Ok(object_end)
    }

    fn ensure_scope_path(
        &mut self,
        path: Option<AmlResolvedNamePath>,
    ) -> AmlResult<Option<AmlNamespaceNodeId>> {
        match path {
            None => Ok(Some(Self::ROOT_NODE_ID)),
            Some(path) if path.segment_count() == 0 => Ok(Some(Self::ROOT_NODE_ID)),
            Some(path) => Ok(Some(self.ensure_scope_like_path(path)?)),
        }
    }

    fn ensure_scope_like_path(
        &mut self,
        path: AmlResolvedNamePath,
    ) -> AmlResult<AmlNamespaceNodeId> {
        if path.segment_count() == 0 {
            return Ok(Self::ROOT_NODE_ID);
        }

        let mut current_id = Self::ROOT_NODE_ID;
        let mut depth = 1_u8;
        while depth <= path.segment_count() {
            let prefix = path.prefix(depth).ok_or_else(AmlError::invalid_name)?;
            if let Some(existing) = self.find_record(prefix) {
                if !scope_capable(existing.descriptor.kind) {
                    return Err(AmlError::namespace_conflict());
                }
                current_id = existing.descriptor.id;
            } else {
                let parent_path = prefix.parent();
                let parent = self.ensure_scope_path(parent_path)?;
                current_id = self.insert_unique_record(
                    prefix,
                    parent,
                    AmlObjectKind::Scope,
                    None,
                    AmlNamespaceNodePayload::None,
                )?;
            }
            depth += 1;
        }

        Ok(current_id)
    }

    fn insert_unique_record(
        &mut self,
        path: AmlResolvedNamePath,
        parent: Option<AmlNamespaceNodeId>,
        kind: AmlObjectKind,
        body: Option<AmlCodeLocation>,
        payload: AmlNamespaceNodePayload,
    ) -> AmlResult<AmlNamespaceNodeId> {
        if self.find_record(path).is_some() {
            return Err(AmlError::namespace_conflict());
        }

        let id = self.next_node_id();
        self.insert_record_with_id(
            AmlNamespaceNodeDescriptor {
                id,
                parent,
                kind,
                path,
            },
            body,
            payload,
        )?;
        Ok(id)
    }

    fn insert_record_with_id(
        &mut self,
        descriptor: AmlNamespaceNodeDescriptor,
        body: Option<AmlCodeLocation>,
        payload: AmlNamespaceNodePayload,
    ) -> AmlResult<()> {
        self.push_record(descriptor, body, payload)?;
        Ok(())
    }

    fn push_record(
        &mut self,
        descriptor: AmlNamespaceNodeDescriptor,
        body: Option<AmlCodeLocation>,
        payload: AmlNamespaceNodePayload,
    ) -> AmlResult<AmlNamespaceNodeId> {
        if self.len >= self.storage.len() {
            return Err(AmlError::overflow());
        }

        self.storage[self.len].write(AmlNamespaceLoadRecord {
            descriptor,
            body,
            payload,
        });
        self.len += 1;
        self.next_id = self.next_id.max(descriptor.id.0.saturating_add(1));
        Ok(descriptor.id)
    }

    fn next_node_id(&mut self) -> AmlNamespaceNodeId {
        let id = AmlNamespaceNodeId(self.next_id);
        self.next_id += 1;
        id
    }

    fn find_record(&self, path: AmlResolvedNamePath) -> Option<&AmlNamespaceLoadRecord> {
        let mut index = 0_usize;
        while index < self.len {
            let record = unsafe { self.storage[index].assume_init_ref() };
            if record.descriptor.path == path {
                return Some(record);
            }
            index += 1;
        }
        None
    }

    fn find_node_id(&self, path: AmlResolvedNamePath) -> Option<AmlNamespaceNodeId> {
        self.find_record(path).map(|record| record.descriptor.id)
    }

    fn loaded_namespace(&self) -> AmlLoadedNamespace<'_, 'plan> {
        let records = unsafe {
            slice::from_raw_parts(
                self.storage.as_ptr().cast::<AmlNamespaceLoadRecord>(),
                self.len,
            )
        };
        AmlLoadedNamespace {
            namespace: AmlNamespace {
                state: AmlNamespaceState::DefinitionBlocksLoaded,
                node_count: self.len as u32,
            },
            records,
            blocks: self.plan.blocks,
        }
    }

    fn evaluate_load_time_term_arg(
        &self,
        bytes: &[u8],
        current_scope_path: AmlResolvedNamePath,
    ) -> AmlResult<(u64, usize)> {
        let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
        match opcode {
            0x00 => Ok((0, 1)),
            0x01 => Ok((1, 1)),
            0xff => Ok((u64::MAX, 1)),
            0x0a => Ok((
                u64::from(bytes.get(1).copied().ok_or_else(AmlError::truncated)?),
                2,
            )),
            0x0b => {
                let raw = bytes.get(1..3).ok_or_else(AmlError::truncated)?;
                Ok((u64::from(u16::from_le_bytes([raw[0], raw[1]])), 3))
            }
            0x0c => {
                let raw = bytes.get(1..5).ok_or_else(AmlError::truncated)?;
                Ok((
                    u64::from(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
                    5,
                ))
            }
            0x0e => {
                let raw = bytes.get(1..9).ok_or_else(AmlError::truncated)?;
                Ok((
                    u64::from_le_bytes([
                        raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
                    ]),
                    9,
                ))
            }
            0x92 => {
                let (value, consumed) =
                    self.evaluate_load_time_term_arg(&bytes[1..], current_scope_path)?;
                Ok(((value == 0) as u64, 1 + consumed))
            }
            0x93..=0x95 => {
                let (lhs, lhs_consumed) =
                    self.evaluate_load_time_term_arg(&bytes[1..], current_scope_path)?;
                let (rhs, rhs_consumed) = self.evaluate_load_time_term_arg(
                    bytes
                        .get(1 + lhs_consumed..)
                        .ok_or_else(AmlError::truncated)?,
                    current_scope_path,
                )?;
                let value = match opcode {
                    0x93 => lhs == rhs,
                    0x94 => lhs > rhs,
                    0x95 => lhs < rhs,
                    _ => false,
                };
                Ok((u64::from(value), 1 + lhs_consumed + rhs_consumed))
            }
            b'\\' | b'^' | b'_' | b'A'..=b'Z' => {
                let encoded = AmlEncodedNameString::parse(bytes)?;
                let path = self
                    .loaded_namespace()
                    .resolve_lookup_path(current_scope_path, encoded)?;
                let value = match self.find_record(path).map(|record| record.payload) {
                    Some(AmlNamespaceNodePayload::NameInteger(value)) => value,
                    _ => return Err(AmlError::unsupported()),
                };
                Ok((value, usize::from(encoded.consumed_bytes)))
            }
            _ => Err(AmlError::unsupported()),
        }
    }

    fn parse_field_entries(
        &mut self,
        object_bytes: &[u8],
        start_cursor: usize,
        object_end: usize,
        current_scope_path: AmlResolvedNamePath,
        current_scope_id: AmlNamespaceNodeId,
        region_id: Option<AmlNamespaceNodeId>,
        flags: u8,
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
                    let mut field_path = current_scope_path;
                    field_path.push(seg)?;
                    let parent_id = Some(current_scope_id);
                    let node_id = self.next_node_id();
                    let descriptor = AmlFieldDescriptor {
                        node: node_id,
                        region: region_id,
                        bit_offset: current_bit_offset,
                        bit_width: width.value,
                        access: decode_field_access(flags),
                        update: decode_field_update(flags),
                    };
                    self.insert_record_with_id(
                        AmlNamespaceNodeDescriptor {
                            id: node_id,
                            parent: parent_id,
                            kind: AmlObjectKind::Field,
                            path: field_path,
                        },
                        None,
                        AmlNamespaceNodePayload::Field(descriptor),
                    )?;
                    current_bit_offset = current_bit_offset.saturating_add(width.value);
                    cursor += 4 + usize::from(width.encoded_bytes);
                }
            }
        }
        Ok(())
    }
}

fn scope_capable(kind: AmlObjectKind) -> bool {
    matches!(
        kind,
        AmlObjectKind::Scope
            | AmlObjectKind::Device
            | AmlObjectKind::Processor
            | AmlObjectKind::PowerResource
            | AmlObjectKind::ThermalZone
    )
}

fn classify_method_kind(path: AmlResolvedNamePath) -> AmlMethodKind {
    let Some(last) = path.last_segment() else {
        return AmlMethodKind::Ordinary;
    };
    match last.bytes() {
        [b'_', b'I', b'N', b'I'] => AmlMethodKind::Initialize,
        [b'_', b'S', b'T', b'A'] => AmlMethodKind::Status,
        [b'_', b'R', b'E', b'G'] => AmlMethodKind::RegionAvailability,
        [b'_', b'Q', _, _] => AmlMethodKind::NotificationQuery,
        [b'_', b'L', hi, lo] | [b'_', b'E', hi, lo]
            if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() =>
        {
            AmlMethodKind::EventHandler
        }
        _ => AmlMethodKind::Ordinary,
    }
}

fn map_address_space(value: u8) -> AmlAddressSpaceId {
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

fn decode_field_access(flags: u8) -> AmlFieldAccessKind {
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

fn decode_field_update(flags: u8) -> AmlFieldUpdateKind {
    match (flags >> 5) & 0b11 {
        0b01 => AmlFieldUpdateKind::WriteAsOnes,
        0b10 => AmlFieldUpdateKind::WriteAsZeros,
        _ => AmlFieldUpdateKind::Preserve,
    }
}

fn parse_term_arg_value(bytes: &[u8]) -> AmlResult<(Option<u64>, usize)> {
    let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
    match opcode {
        0x00 => Ok((Some(0), 1)),
        0x01 => Ok((Some(1), 1)),
        0xff => Ok((Some(u64::MAX), 1)),
        0x0a => Ok((
            Some(u64::from(
                bytes.get(1).copied().ok_or_else(AmlError::truncated)?,
            )),
            2,
        )),
        0x0b => {
            let raw = bytes.get(1..3).ok_or_else(AmlError::truncated)?;
            Ok((Some(u64::from(u16::from_le_bytes([raw[0], raw[1]]))), 3))
        }
        0x0c => {
            let raw = bytes.get(1..5).ok_or_else(AmlError::truncated)?;
            Ok((
                Some(u64::from(u32::from_le_bytes([
                    raw[0], raw[1], raw[2], raw[3],
                ]))),
                5,
            ))
        }
        0x0e => {
            let raw = bytes.get(1..9).ok_or_else(AmlError::truncated)?;
            Ok((
                Some(u64::from_le_bytes([
                    raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
                ])),
                9,
            ))
        }
        b'\\' | b'^' | b'_' | b'A'..=b'Z' => Ok((
            None,
            usize::from(AmlEncodedNameString::parse(bytes)?.consumed_bytes),
        )),
        _ => Err(AmlError::new(
            crate::aml::AmlErrorKind::Unsupported,
            "unsupported aml constant/name term arg",
        )),
    }
}

fn parse_name_initializer(bytes: &[u8]) -> AmlResult<(Option<u64>, usize)> {
    parse_term_arg(bytes)
}

fn parse_term_arg(bytes: &[u8]) -> AmlResult<(Option<u64>, usize)> {
    match parse_term_arg_value(bytes) {
        Ok(parsed) => Ok(parsed),
        Err(error) if error.kind == AmlError::unsupported().kind => {
            let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
            match opcode {
                0x11..=0x13 | 0x0d => parse_data_object(bytes),
                0x70 => {
                    let (_, value_consumed) = parse_term_arg(&bytes[1..])?;
                    let target_consumed = parse_target(
                        bytes
                            .get(1 + value_consumed..)
                            .ok_or_else(AmlError::truncated)?,
                    )?;
                    Ok((None, 1 + value_consumed + target_consumed))
                }
                0x72 | 0x74 | 0x79 | 0x7a | 0x7b | 0x7d | 0x7f => {
                    let (_, lhs_consumed) = parse_term_arg(&bytes[1..])?;
                    let (_, rhs_consumed) = parse_term_arg(
                        bytes
                            .get(1 + lhs_consumed..)
                            .ok_or_else(AmlError::truncated)?,
                    )?;
                    let target_offset = 1 + lhs_consumed + rhs_consumed;
                    let target_consumed =
                        parse_target(bytes.get(target_offset..).ok_or_else(AmlError::truncated)?)?;
                    Ok((None, target_offset + target_consumed))
                }
                0x93..=0x95 => {
                    let (_, lhs_consumed) = parse_term_arg(&bytes[1..])?;
                    let (_, rhs_consumed) = parse_term_arg(
                        bytes
                            .get(1 + lhs_consumed..)
                            .ok_or_else(AmlError::truncated)?,
                    )?;
                    Ok((None, 1 + lhs_consumed + rhs_consumed))
                }
                0x75 | 0x76 => {
                    let target_consumed = parse_target(&bytes[1..])?;
                    Ok((None, 1 + target_consumed))
                }
                0x80 | 0x92 => {
                    let (_, consumed) = parse_term_arg(&bytes[1..])?;
                    Ok((None, 1 + consumed))
                }
                _ => Err(AmlError::new(
                    crate::aml::AmlErrorKind::Unsupported,
                    "unsupported aml composite term arg",
                )),
            }
        }
        Err(error) => Err(error),
    }
}

fn parse_data_object(bytes: &[u8]) -> AmlResult<(Option<u64>, usize)> {
    let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
    match opcode {
        0x0d => {
            let Some(length) = bytes[1..].iter().position(|byte| *byte == 0) else {
                return Err(AmlError::truncated());
            };
            Ok((None, 2 + length))
        }
        0x11..=0x13 => {
            let pkg = AmlPkgLength::parse(&bytes[1..])?;
            Ok((None, 1 + pkg.value as usize))
        }
        _ => Err(AmlError::new(
            crate::aml::AmlErrorKind::Unsupported,
            "unsupported aml name initializer",
        )),
    }
}

fn parse_target(bytes: &[u8]) -> AmlResult<usize> {
    let opcode = *bytes.first().ok_or_else(AmlError::truncated)?;
    match opcode {
        0x00 => Ok(1),
        0x60..=0x6e => Ok(1),
        0x5b => {
            let sub = *bytes.get(1).ok_or_else(AmlError::truncated)?;
            match sub {
                0x31 => Ok(2),
                _ => Err(AmlError::new(
                    crate::aml::AmlErrorKind::Unsupported,
                    "unsupported aml target form",
                )),
            }
        }
        b'\\' | b'^' | b'_' | b'A'..=b'Z' => Ok(usize::from(
            AmlEncodedNameString::parse(bytes)?.consumed_bytes,
        )),
        _ => Err(AmlError::new(
            crate::aml::AmlErrorKind::Unsupported,
            "unsupported aml target form",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pal::hal::acpi::Dsdt;
    use std::boxed::Box;
    use std::vec::Vec;

    fn definition_block(payload: &'static [u8]) -> AmlDefinitionBlock<'static> {
        let bytes = {
            let mut table = Vec::from([0_u8; 36]);
            table[0..4].copy_from_slice(b"DSDT");
            table[4..8].copy_from_slice(&((36 + payload.len()) as u32).to_le_bytes());
            table[8] = 2;
            table[10..16].copy_from_slice(b"FUSION");
            table[16..24].copy_from_slice(b"AMLLOAD ");
            table.extend_from_slice(payload);
            let checksum =
                (!table.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
            table[9] = checksum;
            Box::leak(table.into_boxed_slice())
        };
        AmlDefinitionBlock::from_dsdt(Dsdt::parse(bytes).unwrap()).unwrap()
    }

    #[test]
    fn namespace_loader_surfaces_scope_name_method_opregion_and_fields() {
        let payload: &[u8] = &[
            0x10, 0x33, b'\\', b'_', b'S', b'B', b'_', // Scope(\_SB)
            0x08, b'F', b'O', b'O', b'0', 0x0a, 0x01, // Name(FOO0, 1)
            0x14, 0x08, b'_', b'S', b'T', b'A', 0x00, 0xa4, 0x01, // Method(_STA)
            0x5b, 0x80, b'E', b'C', b'O', b'R', 0x03, 0x0a, 0x10, 0x0a, 0x20, // OpRegion
            0x5b, 0x81, 0x10, b'E', b'C', b'O', b'R', 0x01, b'S', b'T', b'0', b'0', 0x08, b'S',
            b'T', b'0', b'1', 0x08, // Field
        ];
        let block = definition_block(payload);
        let plan =
            AmlNamespaceLoadPlan::from_definition_blocks(AmlDefinitionBlockSet::new(block, &[]));
        let mut storage = [MaybeUninit::<AmlNamespaceLoadRecord>::uninit(); 16];
        let loaded = plan.load_into(&mut storage).expect("namespace should load");

        assert!(loaded.records.iter().any(|record| {
            record.descriptor.kind == AmlObjectKind::Name
                && record.descriptor.path.last_segment().unwrap().bytes() == *b"FOO0"
        }));
        assert!(
            loaded
                .records
                .iter()
                .any(|record| matches!(record.payload, AmlNamespaceNodePayload::Method(_)))
        );
        assert!(
            loaded
                .records
                .iter()
                .any(|record| matches!(record.payload, AmlNamespaceNodePayload::OpRegion(_)))
        );
        assert_eq!(
            loaded
                .records
                .iter()
                .filter(|record| record.descriptor.kind == AmlObjectKind::Field)
                .count(),
            2
        );
    }

    #[test]
    fn namespace_loader_derives_method_kind_from_special_names() {
        let payload: &[u8] = &[
            0x10, 0x0f, b'\\', b'_', b'S', b'B', b'_', 0x14, 0x08, b'_', b'Q', b'4', b'2', 0x00,
            0xa4, 0x00,
        ];
        let block = definition_block(payload);
        let plan =
            AmlNamespaceLoadPlan::from_definition_blocks(AmlDefinitionBlockSet::new(block, &[]));
        let mut storage = [MaybeUninit::<AmlNamespaceLoadRecord>::uninit(); 8];
        let loaded = plan.load_into(&mut storage).expect("namespace should load");
        let method = loaded
            .records
            .iter()
            .find_map(|record| match record.payload {
                AmlNamespaceNodePayload::Method(method) => Some(method),
                _ => None,
            })
            .expect("method should exist");
        assert_eq!(method.kind, AmlMethodKind::NotificationQuery);
    }
}
