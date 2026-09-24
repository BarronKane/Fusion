use super::{
    DomainRegistry,
    CourierPlan,
    CourierId,
    ChildCourierLaunchRecord,
    CourierRecord,
    CourierMetadataEntry,
    CourierMetadataSubject,
    FiberId,
    ContextId,
    CourierObligationRecord,
    CourierObligationId,
    CourierFiberRecord,
    CourierMetadata,
    ContextBaseContract,
    CourierBaseContract,
    CourierSupport,
    CourierVisibilityControlContract,
    CourierVisibility,
    ContextProjectionKind,
    ContextRecord,
    ContextSupport,
};

/// Borrowed view of one courier inside the fixed-capacity domain registry.
#[derive(Debug, Clone, Copy)]
pub struct CourierHandle<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> {
    pub(super) registry: &'registry DomainRegistry<
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >,
    pub(super) index: usize,
}

impl<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>
    CourierHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    #[must_use]
    pub const fn visible_contexts(
        self,
    ) -> VisibleContexts<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    > {
        VisibleContexts {
            courier: self,
            next_visible: 0,
            next_context: 0,
        }
    }

    #[must_use]
    pub const fn plan(self) -> CourierPlan {
        self.record().descriptor.plan
    }

    #[must_use]
    pub const fn parent_courier(self) -> Option<CourierId> {
        self.record().parent
    }

    #[must_use]
    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is violated.
    pub const fn launch_record(self) -> Option<&'registry ChildCourierLaunchRecord<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.launch.as_ref()
    }

    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    pub fn pedigree<const MAX_DEPTH: usize>(
        self,
    ) -> Result<crate::courier::CourierPedigree<'a, MAX_DEPTH>, crate::domain::DomainError> {
        self.registry.courier_pedigree(self.record().descriptor.id)
    }

    #[must_use]
    pub const fn scope_role(self) -> crate::courier::CourierScopeRole {
        self.record().descriptor.scope_role
    }

    #[must_use]
    pub const fn is_context_root(self) -> bool {
        self.scope_role().is_context_root()
    }

    ///
    /// # Errors
    ///
    /// Returns an error if the requested operation cannot be completed.
    pub fn qualified_name<const MAX_CHAIN: usize>(
        self,
    ) -> Result<crate::locator::QualifiedCourierName<'a, MAX_CHAIN>, crate::domain::DomainError>
    {
        self.registry
            .qualified_courier_name::<MAX_CHAIN>(self.record().descriptor.id)
    }

    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is violated.
    pub fn metadata_entries(self) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.app_metadata.iter()
    }

    pub fn courier_metadata(self) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::Courier)
    }

    pub fn fiber_metadata(
        self,
        fiber: FiberId,
    ) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::Fiber(fiber))
    }

    pub fn child_courier_metadata(
        self,
        child: CourierId,
    ) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::ChildCourier(child))
    }

    pub fn context_metadata(
        self,
        context: ContextId,
    ) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::Context(context))
    }

    pub fn async_metadata(self) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        self.metadata_entries_for(CourierMetadataSubject::AsyncLane)
    }

    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is violated.
    pub fn metadata_entries_for(
        self,
        subject: CourierMetadataSubject,
    ) -> impl Iterator<Item = &'registry CourierMetadataEntry<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.app_metadata.entries(subject)
    }

    #[must_use]
    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is violated.
    pub fn metadata_entry_for(
        self,
        subject: CourierMetadataSubject,
        key: &str,
    ) -> Option<&'registry CourierMetadataEntry<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.app_metadata.entry(subject, key)
    }

    #[must_use]
    pub fn courier_metadata_entry(self, key: &str) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::Courier, key)
    }

    #[must_use]
    pub fn fiber_metadata_entry(
        self,
        fiber: FiberId,
        key: &str,
    ) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::Fiber(fiber), key)
    }

    #[must_use]
    pub fn child_courier_metadata_entry(
        self,
        child: CourierId,
        key: &str,
    ) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::ChildCourier(child), key)
    }

    #[must_use]
    pub fn context_metadata_entry(
        self,
        context: ContextId,
        key: &str,
    ) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::Context(context), key)
    }

    #[must_use]
    pub fn async_metadata_entry(self, key: &str) -> Option<&'registry CourierMetadataEntry<'a>> {
        self.metadata_entry_for(CourierMetadataSubject::AsyncLane, key)
    }

    #[must_use]
    pub fn child_courier_count(self) -> usize {
        self.record().children.len()
    }

    #[must_use]
    pub fn obligation_count(self) -> usize {
        self.record().obligations.len()
    }

    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is violated.
    pub fn obligations(self) -> impl Iterator<Item = &'registry CourierObligationRecord<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.obligations.iter()
    }

    #[must_use]
    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is violated.
    pub fn obligation(
        self,
        obligation: CourierObligationId,
    ) -> Option<&'registry CourierObligationRecord<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.obligations.obligation(obligation)
    }

    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is violated.
    pub fn child_couriers(self) -> impl Iterator<Item = &'registry ChildCourierLaunchRecord<'a>> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.children.iter()
    }

    #[must_use]
    pub fn fiber_count(self) -> usize {
        self.record().fibers.len()
    }

    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is violated.
    pub fn fibers(self) -> impl Iterator<Item = &'registry CourierFiberRecord> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.fibers.iter()
    }

    #[must_use]
    pub const fn runtime_ledger(self) -> crate::courier::CourierRuntimeLedger {
        self.record().runtime
    }

    #[must_use]
    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is violated.
    pub fn fiber(self, fiber: FiberId) -> Option<&'registry CourierFiberRecord> {
        let record: &'registry CourierRecord<
            'a,
            MAX_VISIBLE,
            MAX_CHILDREN,
            MAX_FIBERS,
            MAX_METADATA,
        > = self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers");
        record.fibers.fiber(fiber)
    }

    #[must_use]
    pub const fn metadata(self) -> CourierMetadata<'registry> {
        CourierMetadata {
            id: self.record().descriptor.id,
            name: self.record().descriptor.name,
            scope_role: self.record().descriptor.scope_role,
            support: self.record().support,
        }
    }

    const fn record(
        &self,
    ) -> &CourierRecord<'a, MAX_VISIBLE, MAX_CHILDREN, MAX_FIBERS, MAX_METADATA> {
        self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers")
    }
}

impl<
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> CourierBaseContract
    for CourierHandle<
        '_,
        '_,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    fn courier_id(&self) -> CourierId {
        self.record().descriptor.id
    }

    fn name(&self) -> &str {
        self.record().descriptor.name
    }

    fn courier_support(&self) -> CourierSupport {
        self.record().support
    }
}

impl<
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> CourierVisibilityControlContract
    for CourierHandle<
        '_,
        '_,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    fn visible_context_count(&self) -> usize {
        match self.record().support.visibility {
            CourierVisibility::Full => self.registry.contexts.iter().flatten().count(),
            CourierVisibility::Scoped => self.record().visible.iter().flatten().count(),
        }
    }

    fn can_observe_context(&self, context: ContextId) -> bool {
        match self.record().support.visibility {
            CourierVisibility::Full => self.registry.find_context(context).is_some(),
            CourierVisibility::Scoped => self
                .record()
                .visible
                .iter()
                .any(|slot| slot.is_some_and(|grant| grant.context == context)),
        }
    }
}

/// Borrowed view of one context inside the fixed-capacity domain registry.
#[derive(Debug, Clone, Copy)]
pub struct ContextHandle<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> {
    pub(super) registry: &'registry DomainRegistry<
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >,
    pub(super) index: usize,
    pub(super) projection: ContextProjectionKind,
}

impl<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
>
    ContextHandle<
        '_,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    const fn record(&self) -> &ContextRecord<'a> {
        self.registry.contexts[self.index]
            .as_ref()
            .expect("context handle should only point at live contexts")
    }
}

impl<
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> ContextBaseContract
    for ContextHandle<
        '_,
        '_,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    fn context_id(&self) -> ContextId {
        self.record().descriptor.id
    }

    fn name(&self) -> &str {
        self.record().descriptor.name
    }

    fn context_support(&self) -> ContextSupport {
        let mut support = self.record().support;
        support.projection = self.projection;
        support
    }
}

/// Iterator over only the contexts visible to one courier.
pub struct VisibleContexts<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> {
    courier: CourierHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >,
    next_visible: usize,
    next_context: usize,
}

impl<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
    const MAX_CHILDREN: usize,
    const MAX_FIBERS: usize,
    const MAX_METADATA: usize,
> Iterator
    for VisibleContexts<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >
{
    type Item = ContextHandle<
        'registry,
        'a,
        MAX_COURIERS,
        MAX_CONTEXTS,
        MAX_VISIBLE,
        MAX_CHILDREN,
        MAX_FIBERS,
        MAX_METADATA,
    >;

    fn next(&mut self) -> Option<Self::Item> {
        match self.courier.record().support.visibility {
            CourierVisibility::Full => {
                while self.next_context < self.courier.registry.contexts.len() {
                    let index = self.next_context;
                    self.next_context += 1;
                    if self.courier.registry.contexts[index].is_some() {
                        return Some(ContextHandle {
                            registry: self.courier.registry,
                            index,
                            projection: ContextProjectionKind::Owned,
                        });
                    }
                }
                None
            }
            CourierVisibility::Scoped => {
                while self.next_visible < self.courier.record().visible.len() {
                    let slot_index = self.next_visible;
                    self.next_visible += 1;
                    let Some(grant) = self.courier.record().visible[slot_index] else {
                        continue;
                    };
                    let Some(index) = self.courier.registry.index_of_context(grant.context) else {
                        continue;
                    };
                    return Some(ContextHandle {
                        registry: self.courier.registry,
                        index,
                        projection: grant.projection,
                    });
                }
                None
            }
        }
    }
}
