//! fusion-sys domain registry and native domain/courier/context demonstration.

pub use fusion_pal::sys::domain::*;

use crate::context::{
    ContextBase,
    ContextCaps,
    ContextId,
    ContextImplementationKind,
    ContextKind,
    ContextProjectionKind,
    ContextSupport,
};
use crate::courier::{
    CourierBase,
    CourierCaps,
    CourierImplementationKind,
    CourierSupport,
    CourierVisibility,
    CourierVisibilityControl,
};

/// Static descriptor used to construct one native domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainDescriptor<'a> {
    pub id: DomainId,
    pub name: &'a str,
    pub kind: DomainKind,
    pub caps: DomainCaps,
}

/// Static descriptor used to construct one courier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CourierDescriptor<'a> {
    pub id: CourierId,
    pub name: &'a str,
    pub caps: CourierCaps,
    pub visibility: CourierVisibility,
}

/// Static descriptor used to construct one visible context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextDescriptor<'a> {
    pub id: ContextId,
    pub name: &'a str,
    pub kind: ContextKind,
    pub caps: ContextCaps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DomainRecord<'a> {
    descriptor: DomainDescriptor<'a>,
    support: DomainSupport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContextGrant {
    context: ContextId,
    projection: ContextProjectionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CourierRecord<'a, const MAX_VISIBLE: usize> {
    descriptor: CourierDescriptor<'a>,
    support: CourierSupport,
    visible: [Option<ContextGrant>; MAX_VISIBLE],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContextRecord<'a> {
    descriptor: ContextDescriptor<'a>,
    support: ContextSupport,
}

/// Fixed-capacity native domain registry used to prove the first native object model.
#[derive(Debug, Clone, Copy)]
pub struct DomainRegistry<
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
> {
    domain: DomainRecord<'a>,
    couriers: [Option<CourierRecord<'a, MAX_VISIBLE>>; MAX_COURIERS],
    contexts: [Option<ContextRecord<'a>>; MAX_CONTEXTS],
}

impl<'a, const MAX_COURIERS: usize, const MAX_CONTEXTS: usize, const MAX_VISIBLE: usize>
    DomainRegistry<'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>
{
    /// Creates one fixed-capacity domain registry.
    #[must_use]
    pub fn new(descriptor: DomainDescriptor<'a>) -> Self {
        Self {
            domain: DomainRecord {
                descriptor,
                support: DomainSupport {
                    caps: descriptor.caps,
                    implementation: DomainImplementationKind::Native,
                    kind: descriptor.kind,
                },
            },
            couriers: [None; MAX_COURIERS],
            contexts: [None; MAX_CONTEXTS],
        }
    }

    /// Registers one courier inside the domain.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the ID already exists or storage is exhausted.
    pub fn register_courier(
        &mut self,
        descriptor: CourierDescriptor<'a>,
    ) -> Result<(), DomainError> {
        self.insert_courier(CourierRecord {
            descriptor,
            support: CourierSupport {
                caps: descriptor.caps,
                implementation: CourierImplementationKind::Native,
                domain: self.domain.descriptor.id,
                visibility: descriptor.visibility,
            },
            visible: [None; MAX_VISIBLE],
        })
    }

    /// Registers one owned context under the supplied owning courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the owner does not exist, the ID already exists, or storage is
    /// exhausted.
    pub fn register_context(
        &mut self,
        owner: CourierId,
        descriptor: ContextDescriptor<'a>,
    ) -> Result<(), DomainError> {
        if self.find_courier(owner).is_none() {
            return Err(DomainError::not_found());
        }
        if self.find_context(descriptor.id).is_some() {
            return Err(DomainError::state_conflict());
        }
        let Some(slot) = self.contexts.iter_mut().find(|slot| slot.is_none()) else {
            return Err(DomainError::resource_exhausted());
        };
        *slot = Some(ContextRecord {
            descriptor,
            support: ContextSupport {
                caps: descriptor.caps,
                implementation: ContextImplementationKind::Native,
                domain: self.domain.descriptor.id,
                owner,
                kind: descriptor.kind,
                projection: ContextProjectionKind::Owned,
            },
        });
        Ok(())
    }

    /// Grants one projected context into the target courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier or context does not exist, the context is already
    /// visible, or the visibility table is exhausted.
    pub fn grant_context(
        &mut self,
        courier: CourierId,
        context: ContextId,
        projection: ContextProjectionKind,
    ) -> Result<(), DomainError> {
        if self.find_context(context).is_none() {
            return Err(DomainError::not_found());
        }
        let courier_record = self
            .find_courier_mut(courier)
            .ok_or_else(DomainError::not_found)?;
        if courier_record
            .visible
            .iter()
            .any(|slot| slot.is_some_and(|grant| grant.context == context))
        {
            return Err(DomainError::state_conflict());
        }
        let Some(slot) = courier_record
            .visible
            .iter_mut()
            .find(|slot| slot.is_none())
        else {
            return Err(DomainError::resource_exhausted());
        };
        *slot = Some(ContextGrant {
            context,
            projection,
        });
        Ok(())
    }

    /// Returns a handle to one registered courier.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the courier does not exist.
    pub fn courier(
        &self,
        courier: CourierId,
    ) -> Result<CourierHandle<'_, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>, DomainError> {
        let Some(index) = self.index_of_courier(courier) else {
            return Err(DomainError::not_found());
        };
        Ok(CourierHandle {
            registry: self,
            index,
        })
    }

    /// Returns one registered context handle.
    ///
    /// # Errors
    ///
    /// Returns an honest error when the context does not exist.
    pub fn context(
        &self,
        context: ContextId,
    ) -> Result<ContextHandle<'_, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>, DomainError> {
        let Some(index) = self.index_of_context(context) else {
            return Err(DomainError::not_found());
        };
        Ok(ContextHandle {
            registry: self,
            index,
            projection: ContextProjectionKind::Owned,
        })
    }

    fn insert_courier(
        &mut self,
        record: CourierRecord<'a, MAX_VISIBLE>,
    ) -> Result<(), DomainError> {
        if self.find_courier(record.descriptor.id).is_some() {
            return Err(DomainError::state_conflict());
        }
        let Some(slot) = self.couriers.iter_mut().find(|slot| slot.is_none()) else {
            return Err(DomainError::resource_exhausted());
        };
        *slot = Some(record);
        Ok(())
    }

    fn find_courier(&self, courier: CourierId) -> Option<&CourierRecord<'a, MAX_VISIBLE>> {
        self.couriers
            .iter()
            .flatten()
            .find(|record| record.descriptor.id == courier)
    }

    fn find_courier_mut(
        &mut self,
        courier: CourierId,
    ) -> Option<&mut CourierRecord<'a, MAX_VISIBLE>> {
        self.couriers
            .iter_mut()
            .flatten()
            .find(|record| record.descriptor.id == courier)
    }

    fn index_of_courier(&self, courier: CourierId) -> Option<usize> {
        self.couriers
            .iter()
            .position(|slot| slot.is_some_and(|record| record.descriptor.id == courier))
    }

    fn find_context(&self, context: ContextId) -> Option<&ContextRecord<'a>> {
        self.contexts
            .iter()
            .flatten()
            .find(|record| record.descriptor.id == context)
    }

    fn index_of_context(&self, context: ContextId) -> Option<usize> {
        self.contexts
            .iter()
            .position(|slot| slot.is_some_and(|record| record.descriptor.id == context))
    }
}

impl<'a, const MAX_COURIERS: usize, const MAX_CONTEXTS: usize, const MAX_VISIBLE: usize> DomainBase
    for DomainRegistry<'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>
{
    fn domain_id(&self) -> DomainId {
        self.domain.descriptor.id
    }

    fn name(&self) -> &str {
        self.domain.descriptor.name
    }

    fn domain_support(&self) -> DomainSupport {
        self.domain.support
    }
}

/// Borrowed view of one courier inside the fixed-capacity domain registry.
#[derive(Debug, Clone, Copy)]
pub struct CourierHandle<
    'registry,
    'a,
    const MAX_COURIERS: usize,
    const MAX_CONTEXTS: usize,
    const MAX_VISIBLE: usize,
> {
    registry: &'registry DomainRegistry<'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>,
    index: usize,
}

impl<'registry, 'a, const MAX_COURIERS: usize, const MAX_CONTEXTS: usize, const MAX_VISIBLE: usize>
    CourierHandle<'registry, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>
{
    #[must_use]
    pub fn visible_contexts(
        self,
    ) -> VisibleContexts<'registry, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE> {
        VisibleContexts {
            courier: self,
            next_visible: 0,
            next_context: 0,
        }
    }

    fn record(&self) -> &CourierRecord<'a, MAX_VISIBLE> {
        self.registry.couriers[self.index]
            .as_ref()
            .expect("courier handle should only point at live couriers")
    }
}

impl<'registry, 'a, const MAX_COURIERS: usize, const MAX_CONTEXTS: usize, const MAX_VISIBLE: usize>
    CourierBase for CourierHandle<'registry, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>
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

impl<'registry, 'a, const MAX_COURIERS: usize, const MAX_CONTEXTS: usize, const MAX_VISIBLE: usize>
    CourierVisibilityControl
    for CourierHandle<'registry, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>
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
> {
    registry: &'registry DomainRegistry<'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>,
    index: usize,
    projection: ContextProjectionKind,
}

impl<'registry, 'a, const MAX_COURIERS: usize, const MAX_CONTEXTS: usize, const MAX_VISIBLE: usize>
    ContextHandle<'registry, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>
{
    fn record(&self) -> &ContextRecord<'a> {
        self.registry.contexts[self.index]
            .as_ref()
            .expect("context handle should only point at live contexts")
    }
}

impl<'registry, 'a, const MAX_COURIERS: usize, const MAX_CONTEXTS: usize, const MAX_VISIBLE: usize>
    ContextBase for ContextHandle<'registry, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>
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
> {
    courier: CourierHandle<'registry, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>,
    next_visible: usize,
    next_context: usize,
}

impl<'registry, 'a, const MAX_COURIERS: usize, const MAX_CONTEXTS: usize, const MAX_VISIBLE: usize>
    Iterator for VisibleContexts<'registry, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>
{
    type Item = ContextHandle<'registry, 'a, MAX_COURIERS, MAX_CONTEXTS, MAX_VISIBLE>;

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

#[cfg(all(test, feature = "std", not(target_os = "none")))]
mod tests {
    use super::*;

    const DOMAIN_ID: DomainId = DomainId::new(0x5056_4153);
    const PRIMARY_COURIER: CourierId = CourierId::new(1);
    const SCOPED_COURIER: CourierId = CourierId::new(2);
    const FIBER_CONTEXT: ContextId = ContextId::new(0x100);
    const BLOCK_CONTEXT: ContextId = ContextId::new(0x101);

    #[test]
    fn couriers_enumerate_only_their_visible_contexts() {
        let mut registry: DomainRegistry<'_, 4, 8, 4> = DomainRegistry::new(DomainDescriptor {
            id: DOMAIN_ID,
            name: "pvas",
            kind: DomainKind::NativeSubstrate,
            caps: DomainCaps::COURIER_REGISTRY
                | DomainCaps::CONTEXT_REGISTRY
                | DomainCaps::COURIER_VISIBILITY,
        });

        registry
            .register_courier(CourierDescriptor {
                id: PRIMARY_COURIER,
                name: "primary",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS
                    | CourierCaps::PROJECT_CONTEXTS
                    | CourierCaps::SPAWN_SUB_FIBERS
                    | CourierCaps::DEBUG_CHANNEL,
                visibility: CourierVisibility::Full,
            })
            .expect("primary courier should register");
        registry
            .register_context(
                PRIMARY_COURIER,
                ContextDescriptor {
                    id: FIBER_CONTEXT,
                    name: "primary.main",
                    kind: ContextKind::FiberMetadata,
                    caps: ContextCaps::PROJECTABLE | ContextCaps::CONTROL_ENDPOINT,
                },
            )
            .expect("fiber metadata context should register");
        registry
            .register_context(
                PRIMARY_COURIER,
                ContextDescriptor {
                    id: BLOCK_CONTEXT,
                    name: "nvme0n1p1",
                    kind: ContextKind::StorageEndpoint,
                    caps: ContextCaps::PROJECTABLE | ContextCaps::CHANNEL_BACKED,
                },
            )
            .expect("block context should register");
        registry
            .register_courier(CourierDescriptor {
                id: SCOPED_COURIER,
                name: "scoped",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Scoped,
            })
            .expect("scoped courier should register");
        registry
            .grant_context(SCOPED_COURIER, BLOCK_CONTEXT, ContextProjectionKind::Alias)
            .expect("scoped courier should receive one projected block context");

        let primary = registry
            .courier(PRIMARY_COURIER)
            .expect("primary courier should exist");
        assert_eq!(
            primary.courier_support().visibility,
            CourierVisibility::Full
        );
        assert_eq!(primary.visible_context_count(), 2);

        let scoped = registry
            .courier(SCOPED_COURIER)
            .expect("scoped courier should exist");
        assert_eq!(
            scoped.courier_support().visibility,
            CourierVisibility::Scoped
        );
        assert_eq!(scoped.visible_context_count(), 1);
        assert!(!scoped.can_observe_context(FIBER_CONTEXT));
        assert!(scoped.can_observe_context(BLOCK_CONTEXT));

        let visible: [Option<&str>; 2] = {
            let mut names = [None; 2];
            for (index, context) in scoped.visible_contexts().enumerate() {
                names[index] = Some(context.record().descriptor.name);
                assert_eq!(
                    context.context_support().projection,
                    ContextProjectionKind::Alias
                );
            }
            names
        };
        assert_eq!(visible, [Some("nvme0n1p1"), None]);
    }

    #[test]
    fn duplicate_courier_ids_are_rejected() {
        let mut registry: DomainRegistry<'_, 4, 4, 4> = DomainRegistry::new(DomainDescriptor {
            id: DOMAIN_ID,
            name: "pvas",
            kind: DomainKind::NativeSubstrate,
            caps: DomainCaps::COURIER_REGISTRY | DomainCaps::COURIER_VISIBILITY,
        });
        registry
            .register_courier(CourierDescriptor {
                id: PRIMARY_COURIER,
                name: "primary",
                caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
                visibility: CourierVisibility::Full,
            })
            .expect("primary courier should register");

        let result = registry.register_courier(CourierDescriptor {
            id: PRIMARY_COURIER,
            name: "duplicate",
            caps: CourierCaps::ENUMERATE_VISIBLE_CONTEXTS,
            visibility: CourierVisibility::Scoped,
        });
        assert!(matches!(
            result,
            Err(error) if error.kind() == DomainErrorKind::StateConflict
        ));
    }
}
