//! Dynamic hardware-discovery support for the `fusion-firmware::sys::hal` lane.
//!
//! The ACPI lane already knew how to:
//! - parse validated tables,
//! - load AML,
//! - realize one matched platform,
//! - and activate AML lifecycle against a resolved namespace.
//!
//! What it did not know how to do was the boring, necessary middle step:
//! - start from an `XSDT`,
//! - resolve table pointers through one firmware-owned mapping surface,
//! - find the `FADT`,
//! - follow it to the `DSDT`,
//! - collect secondary definition blocks,
//! - and feed the result into the existing realization path.
//!
//! That omission kept the AML story tasteful and useless. This module fixes the
//! latter part.

use core::mem::MaybeUninit;
use core::slice;

use crate::aml::{
    AmlBackendVerificationIssue,
    AmlDefinitionBlock,
    AmlNamespaceLoadRecord,
    AmlRegionAccessHost,
    AmlRuntimeState,
};
use crate::pal::hal::acpi::{
    AcpiError,
    AcpiErrorKind,
    AcpiPlatformFingerprint,
    AcpiRealizationError,
    AcpiSignature,
    AcpiTableView,
    Dsdt,
    Fadt,
    RealizedAcpiPlatformWithAml,
    Xsdt,
    realize_platform_from_definition_tables_with_aml,
};

/// Firmware-owned physical table lookup surface used by ACPI discovery.
pub trait AcpiPhysicalTableResolver {
    /// Resolves one physical ACPI table address into borrowed bytes.
    ///
    /// # Errors
    ///
    /// Returns one honest error when the table cannot be resolved or mapped.
    fn resolve_table_bytes<'a>(
        &'a self,
        physical_address: u64,
    ) -> Result<&'a [u8], AcpiRealizationError>;
}

/// Borrowed discovery result for the ACPI definition-table subset needed by AML bring-up.
#[derive(Clone, Copy, Debug)]
pub struct AcpiDefinitionTableDiscovery<'a> {
    xsdt: Xsdt<'a>,
    fadt: Fadt<'a>,
    dsdt: Dsdt<'a>,
    secondary_definition_tables: &'a [AcpiTableView<'a>],
}

impl<'a> AcpiDefinitionTableDiscovery<'a> {
    #[must_use]
    pub const fn xsdt(self) -> Xsdt<'a> {
        self.xsdt
    }

    #[must_use]
    pub const fn fadt(self) -> Fadt<'a> {
        self.fadt
    }

    #[must_use]
    pub const fn dsdt(self) -> Dsdt<'a> {
        self.dsdt
    }

    #[must_use]
    pub const fn secondary_definition_tables(self) -> &'a [AcpiTableView<'a>] {
        self.secondary_definition_tables
    }
}

/// Caller-owned storage bundle for ACPI AML bring-up from discovered definition tables.
#[derive(Debug)]
pub struct AcpiAmlBringupStorage<'records, 'tables, 'issues> {
    pub secondary_definition_tables: &'tables mut [MaybeUninit<AcpiTableView<'tables>>],
    pub definition_blocks: &'tables mut [MaybeUninit<AmlDefinitionBlock<'tables>>],
    pub namespace_records: &'records mut [MaybeUninit<AmlNamespaceLoadRecord>],
    pub verification_issues: &'issues mut [MaybeUninit<AmlBackendVerificationIssue>],
}

impl<'records, 'tables, 'issues> AcpiAmlBringupStorage<'records, 'tables, 'issues> {
    #[must_use]
    pub const fn new(
        secondary_definition_tables: &'tables mut [MaybeUninit<AcpiTableView<'tables>>],
        definition_blocks: &'tables mut [MaybeUninit<AmlDefinitionBlock<'tables>>],
        namespace_records: &'records mut [MaybeUninit<AmlNamespaceLoadRecord>],
        verification_issues: &'issues mut [MaybeUninit<AmlBackendVerificationIssue>],
    ) -> Self {
        Self {
            secondary_definition_tables,
            definition_blocks,
            namespace_records,
            verification_issues,
        }
    }
}

/// Result of hardware discovery plus AML-backed ACPI realization.
#[derive(Debug)]
pub struct DiscoveredAcpiPlatformWithAml<'tables, 'issues> {
    definition_tables: AcpiDefinitionTableDiscovery<'tables>,
    realized: RealizedAcpiPlatformWithAml<'issues>,
}

impl<'tables, 'issues> DiscoveredAcpiPlatformWithAml<'tables, 'issues> {
    #[must_use]
    pub const fn definition_tables(&self) -> AcpiDefinitionTableDiscovery<'tables> {
        self.definition_tables
    }

    #[must_use]
    pub const fn realized(&self) -> &RealizedAcpiPlatformWithAml<'issues> {
        &self.realized
    }

    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        AcpiDefinitionTableDiscovery<'tables>,
        RealizedAcpiPlatformWithAml<'issues>,
    ) {
        (self.definition_tables, self.realized)
    }
}

struct SecondaryDefinitionTableWriter<'a, 'tables> {
    storage: &'a mut [MaybeUninit<AcpiTableView<'tables>>],
    len: usize,
}

impl<'a, 'tables> SecondaryDefinitionTableWriter<'a, 'tables> {
    fn new(storage: &'a mut [MaybeUninit<AcpiTableView<'tables>>]) -> Self {
        Self { storage, len: 0 }
    }

    fn push(&mut self, table: AcpiTableView<'tables>) -> Result<(), AcpiRealizationError> {
        let Some(slot) = self.storage.get_mut(self.len) else {
            return Err(AcpiRealizationError::resource_exhausted());
        };
        slot.write(table);
        self.len += 1;
        Ok(())
    }

    fn finish(self) -> &'a [AcpiTableView<'tables>] {
        unsafe {
            slice::from_raw_parts(
                self.storage.as_ptr().cast::<AcpiTableView<'tables>>(),
                self.len,
            )
        }
    }
}

/// Resolves `FADT`, `DSDT`, and secondary AML definition tables from one validated `XSDT`.
///
/// # Errors
///
/// Returns one honest error when:
/// - the `XSDT` does not lead to one valid `FADT`,
/// - the `FADT` does not lead to one valid `DSDT`,
/// - secondary definition-table storage is exhausted,
/// - or the resolver cannot surface the pointed-to table bytes.
pub fn discover_definition_tables_from_xsdt<'tables, R>(
    xsdt: Xsdt<'tables>,
    resolver: &'tables R,
    secondary_definition_table_storage: &'tables mut [MaybeUninit<AcpiTableView<'tables>>],
) -> Result<AcpiDefinitionTableDiscovery<'tables>, AcpiRealizationError>
where
    R: AcpiPhysicalTableResolver,
{
    let mut fadt = None;
    let mut secondary_tables =
        SecondaryDefinitionTableWriter::new(secondary_definition_table_storage);

    for table_address in xsdt.entries() {
        let table_bytes = resolver.resolve_table_bytes(table_address)?;
        let table = AcpiTableView::parse(table_bytes).map_err(map_acpi_table_error)?;
        match table.header().signature() {
            AcpiSignature::FADT if fadt.is_none() => {
                fadt = Some(Fadt::parse(table_bytes).map_err(map_acpi_table_error)?);
            }
            AcpiSignature::SSDT | AcpiSignature::PSDT => {
                secondary_tables.push(table)?;
            }
            _ => {}
        }
    }

    let fadt = fadt.ok_or_else(AcpiRealizationError::invalid)?;
    let dsdt_bytes = resolver.resolve_table_bytes(fadt.effective_dsdt_address())?;
    let dsdt = Dsdt::parse(dsdt_bytes).map_err(map_acpi_table_error)?;

    Ok(AcpiDefinitionTableDiscovery {
        xsdt,
        fadt,
        dsdt,
        secondary_definition_tables: secondary_tables.finish(),
    })
}

/// Discovers AML definition tables from one `XSDT`, then realizes and activates the matched ACPI
/// backend against them.
///
/// # Errors
///
/// Returns one honest error when table discovery fails, AML namespace loading fails, backend
/// verification fails, or AML lifecycle activation cannot complete cleanly.
pub fn realize_platform_from_xsdt_with_aml<'records, 'tables, 'issues, R>(
    fingerprint: &AcpiPlatformFingerprint,
    xsdt: Xsdt<'tables>,
    resolver: &'tables R,
    storage: AcpiAmlBringupStorage<'records, 'tables, 'issues>,
    host: &dyn AmlRegionAccessHost,
    runtime: &AmlRuntimeState<'_>,
) -> Result<DiscoveredAcpiPlatformWithAml<'tables, 'issues>, AcpiRealizationError>
where
    R: AcpiPhysicalTableResolver,
{
    let definition_tables =
        discover_definition_tables_from_xsdt(xsdt, resolver, storage.secondary_definition_tables)?;
    let realized = realize_platform_from_definition_tables_with_aml(
        fingerprint,
        definition_tables.dsdt,
        definition_tables.secondary_definition_tables,
        storage.definition_blocks,
        storage.namespace_records,
        host,
        runtime,
        storage.verification_issues,
    )?;
    Ok(DiscoveredAcpiPlatformWithAml {
        definition_tables,
        realized,
    })
}

fn map_acpi_table_error(error: AcpiError) -> AcpiRealizationError {
    match error.kind() {
        AcpiErrorKind::Truncated
        | AcpiErrorKind::InvalidSignature
        | AcpiErrorKind::InvalidChecksum
        | AcpiErrorKind::InvalidLayout => AcpiRealizationError::invalid(),
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use std::boxed::Box;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::vec::Vec;

    use super::*;
    use crate::aml::{
        AmlBackendVerificationIssue,
        AmlDefinitionBlock,
        AmlNamespaceLoadRecord,
        verify_acpi_backend,
    };
    use crate::pal::hal::acpi::{
        AcpiSignature,
        dell_latitude_e6430_fingerprint,
        load_namespace_from_definition_tables,
    };

    const DELL_DSDT_PATH: &str =
        "/volumes/projects/acpi/PVAS-PL1/hw-export-PVAS-PL1-20260405-004625/acpi/dsdt.dat";

    struct StaticAcpiTableResolver {
        tables: BTreeMap<u64, &'static [u8]>,
    }

    impl StaticAcpiTableResolver {
        fn new(entries: &[(u64, &'static [u8])]) -> Self {
            let mut tables = BTreeMap::new();
            for (address, bytes) in entries {
                tables.insert(*address, *bytes);
            }
            Self { tables }
        }
    }

    impl AcpiPhysicalTableResolver for StaticAcpiTableResolver {
        fn resolve_table_bytes<'a>(
            &'a self,
            physical_address: u64,
        ) -> Result<&'a [u8], AcpiRealizationError> {
            self.tables
                .get(&physical_address)
                .copied()
                .ok_or_else(AcpiRealizationError::invalid)
        }
    }

    fn leak_boxed(bytes: Vec<u8>) -> &'static [u8] {
        Box::leak(bytes.into_boxed_slice())
    }

    fn build_xsdt(entries: &[u64]) -> &'static [u8] {
        let mut bytes = vec![0_u8; 36 + entries.len() * 8];
        let length = bytes.len() as u32;
        bytes[0..4].copy_from_slice(b"XSDT");
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
        bytes[8] = 1;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"HWDISCOV");
        for (index, entry) in entries.iter().enumerate() {
            let start = 36 + (index * 8);
            bytes[start..start + 8].copy_from_slice(&entry.to_le_bytes());
        }
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        leak_boxed(bytes)
    }

    fn build_fadt(dsdt_address: u64) -> &'static [u8] {
        let mut bytes = vec![0_u8; 148];
        let length = bytes.len() as u32;
        bytes[0..4].copy_from_slice(b"FACP");
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
        bytes[8] = 6;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"HWDISCOV");
        bytes[40..44].copy_from_slice(&(dsdt_address as u32).to_le_bytes());
        bytes[140..148].copy_from_slice(&dsdt_address.to_le_bytes());
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        leak_boxed(bytes)
    }

    fn build_definition_table(signature: [u8; 4], payload: &[u8]) -> &'static [u8] {
        let mut bytes = vec![0_u8; 36 + payload.len()];
        let length = bytes.len() as u32;
        bytes[0..4].copy_from_slice(&signature);
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
        bytes[8] = 2;
        bytes[10..16].copy_from_slice(b"FUSION");
        bytes[16..24].copy_from_slice(b"HWDISCOV");
        bytes[36..].copy_from_slice(payload);
        let checksum =
            (!bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1);
        bytes[9] = checksum;
        leak_boxed(bytes)
    }

    #[test]
    fn xsdt_discovery_finds_fadt_dsdt_and_secondary_definition_tables() {
        let xsdt_address = 0x1000;
        let fadt_address = 0x2000;
        let dsdt_address = 0x3000;
        let ssdt_address = 0x4000;

        let xsdt = Xsdt::parse(build_xsdt(&[fadt_address, ssdt_address])).unwrap();
        let fadt = build_fadt(dsdt_address);
        let dsdt =
            build_definition_table(*b"DSDT", &[0x10, 0x08, b'\\', b'_', b'S', b'B', b'_', 0x08]);
        let ssdt = build_definition_table(*b"SSDT", &[0x08, b'F', b'O', b'O', b'0', 0x0a, 0x01]);
        let resolver = StaticAcpiTableResolver::new(&[
            (xsdt_address, build_xsdt(&[fadt_address, ssdt_address])),
            (fadt_address, fadt),
            (dsdt_address, dsdt),
            (ssdt_address, ssdt),
        ]);
        let mut secondary_storage = [MaybeUninit::<AcpiTableView<'static>>::uninit(); 4];

        let discovered =
            discover_definition_tables_from_xsdt(xsdt, &resolver, &mut secondary_storage).unwrap();

        assert_eq!(discovered.fadt().effective_dsdt_address(), dsdt_address);
        assert_eq!(
            discovered.dsdt().table().header().signature(),
            AcpiSignature::DSDT
        );
        assert_eq!(discovered.secondary_definition_tables().len(), 1);
        assert_eq!(
            discovered.secondary_definition_tables()[0]
                .header()
                .signature(),
            AcpiSignature::SSDT
        );
    }

    #[test]
    fn xsdt_discovery_loads_and_verifies_dell_namespace_from_captured_dsdt() {
        if !Path::new(DELL_DSDT_PATH).exists() {
            return;
        }

        let dsdt_bytes = leak_boxed(fs::read(DELL_DSDT_PATH).unwrap());
        let fadt_address = 0x2000;
        let dsdt_address = 0x3000;
        let xsdt = Xsdt::parse(build_xsdt(&[fadt_address])).unwrap();
        let fadt = build_fadt(dsdt_address);
        let resolver =
            StaticAcpiTableResolver::new(&[(fadt_address, fadt), (dsdt_address, dsdt_bytes)]);
        let mut secondary_storage = [MaybeUninit::<AcpiTableView<'static>>::uninit(); 8];
        let discovered =
            discover_definition_tables_from_xsdt(xsdt, &resolver, &mut secondary_storage).unwrap();
        let mut definition_storage = [MaybeUninit::<AmlDefinitionBlock<'static>>::uninit(); 8];
        let mut namespace_storage = [MaybeUninit::<AmlNamespaceLoadRecord>::uninit(); 8192];
        let namespace = load_namespace_from_definition_tables(
            discovered.dsdt(),
            discovered.secondary_definition_tables(),
            &mut definition_storage,
            &mut namespace_storage,
        )
        .unwrap();
        let mut issue_storage = [MaybeUninit::<AmlBackendVerificationIssue>::uninit(); 64];
        let report = verify_acpi_backend::<
            fusion_hal::drivers::acpi::vendor::dell::DellLatitudeE6430AcpiHardware,
        >(namespace, 0, &mut issue_storage)
        .unwrap();

        assert_eq!(discovered.fadt().effective_dsdt_address(), dsdt_address);
        assert_eq!(
            discovered.dsdt().table().header().signature(),
            AcpiSignature::DSDT
        );
        assert!(dell_latitude_e6430_fingerprint().product_name == "Latitude E6430");
        assert!(report.is_clean(), "{report:?}");
    }
}
