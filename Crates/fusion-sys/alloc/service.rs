//! Local allocator-domain audit service speaking through one-way channels.

use core::array;

use crate::channel::{ChannelError, ChannelErrorKind, ChannelReceive, ChannelSend, LocalChannel};
use crate::transport::{
    TransportAttachmentControl,
    TransportAttachmentRequest,
    TransportError,
    TransportErrorKind,
};

use super::{
    AllocError,
    AllocErrorKind,
    Allocator,
    AllocatorControlRequest,
    AllocatorControlStatusMessage,
    AllocatorControlStatusProtocol,
    AllocatorControlWriteProtocol,
    AllocatorDomainId,
    AllocatorDomainMetadataMessage,
    AllocatorDomainMetadataProtocol,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PendingStatusStream {
    PoolMembers {
        domain: AllocatorDomainId,
        next_index: usize,
    },
    PoolExtents {
        domain: AllocatorDomainId,
        next_index: usize,
    },
}

/// Error surfaced while constructing or pumping one allocator-domain channel service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocatorChannelServiceError {
    kind: AllocatorChannelServiceErrorKind,
}

impl AllocatorChannelServiceError {
    /// Returns the concrete service error kind.
    #[must_use]
    pub const fn kind(self) -> AllocatorChannelServiceErrorKind {
        self.kind
    }
}

/// Classification of allocator-domain channel service failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllocatorChannelServiceErrorKind {
    Alloc(AllocErrorKind),
    Channel(ChannelErrorKind),
    Transport(TransportErrorKind),
}

impl From<AllocError> for AllocatorChannelServiceError {
    fn from(value: AllocError) -> Self {
        Self {
            kind: AllocatorChannelServiceErrorKind::Alloc(value.kind),
        }
    }
}

impl From<ChannelError> for AllocatorChannelServiceError {
    fn from(value: ChannelError) -> Self {
        Self {
            kind: AllocatorChannelServiceErrorKind::Channel(value.kind()),
        }
    }
}

impl From<TransportError> for AllocatorChannelServiceError {
    fn from(value: TransportError) -> Self {
        Self {
            kind: AllocatorChannelServiceErrorKind::Transport(value.kind()),
        }
    }
}

/// Same-context allocator-domain audit service over local channels.
pub struct AllocatorChannelService<
    'a,
    const DOMAINS: usize,
    const RESOURCES: usize,
    const EXTENTS: usize,
    const METADATA_CAPACITY: usize = 8,
    const CONTROL_CAPACITY: usize = 8,
    const STATUS_CAPACITY: usize = 8,
> {
    allocator: &'a Allocator<DOMAINS, RESOURCES, EXTENTS>,
    domain_ids: [AllocatorDomainId; DOMAINS],
    domain_count: usize,
    next_metadata: usize,
    pending_status: Option<AllocatorControlStatusMessage>,
    pending_stream: Option<PendingStatusStream>,
    metadata_channel: LocalChannel<AllocatorDomainMetadataProtocol, METADATA_CAPACITY>,
    control_channel: LocalChannel<AllocatorControlWriteProtocol, CONTROL_CAPACITY>,
    status_channel: LocalChannel<AllocatorControlStatusProtocol, STATUS_CAPACITY>,
    metadata_producer: usize,
    control_consumer: usize,
    status_producer: usize,
}

impl<
    'a,
    const DOMAINS: usize,
    const RESOURCES: usize,
    const EXTENTS: usize,
    const METADATA_CAPACITY: usize,
    const CONTROL_CAPACITY: usize,
    const STATUS_CAPACITY: usize,
>
    AllocatorChannelService<
        'a,
        DOMAINS,
        RESOURCES,
        EXTENTS,
        METADATA_CAPACITY,
        CONTROL_CAPACITY,
        STATUS_CAPACITY,
    >
{
    /// Creates one local allocator-domain audit service for one allocator.
    ///
    /// # Errors
    ///
    /// Returns an honest channel-transport failure when the local channels cannot be
    /// instantiated or attached.
    pub fn new(
        allocator: &'a Allocator<DOMAINS, RESOURCES, EXTENTS>,
    ) -> Result<Self, AllocatorChannelServiceError> {
        let metadata_channel =
            LocalChannel::<AllocatorDomainMetadataProtocol, METADATA_CAPACITY>::new()?;
        let control_channel =
            LocalChannel::<AllocatorControlWriteProtocol, CONTROL_CAPACITY>::new()?;
        let status_channel =
            LocalChannel::<AllocatorControlStatusProtocol, STATUS_CAPACITY>::new()?;
        let request = TransportAttachmentRequest::same_courier();

        let metadata_producer = metadata_channel.attach_producer(request)?;
        let control_consumer = control_channel.attach_consumer(request)?;
        let status_producer = status_channel.attach_producer(request)?;

        let mut domain_ids = array::from_fn(|_| AllocatorDomainId(u16::MAX));
        let domain_count = allocator.write_domain_ids(&mut domain_ids);

        Ok(Self {
            allocator,
            domain_ids,
            domain_count,
            next_metadata: 0,
            pending_status: None,
            pending_stream: None,
            metadata_channel,
            control_channel,
            status_channel,
            metadata_producer,
            control_consumer,
            status_producer,
        })
    }

    /// Returns the metadata channel for allocator-domain advertisements.
    #[must_use]
    pub const fn metadata_channel(
        &self,
    ) -> &LocalChannel<AllocatorDomainMetadataProtocol, METADATA_CAPACITY> {
        &self.metadata_channel
    }

    /// Returns the control channel for allocator-domain audit requests.
    #[must_use]
    pub const fn control_channel(
        &self,
    ) -> &LocalChannel<AllocatorControlWriteProtocol, CONTROL_CAPACITY> {
        &self.control_channel
    }

    /// Returns the status channel for allocator-domain audit replies.
    #[must_use]
    pub const fn status_channel(
        &self,
    ) -> &LocalChannel<AllocatorControlStatusProtocol, STATUS_CAPACITY> {
        &self.status_channel
    }

    /// Pumps pending metadata and control requests once.
    ///
    /// # Errors
    ///
    /// Returns an honest channel or allocator failure when the service cannot make forward
    /// progress honestly.
    pub fn pump(&mut self) -> Result<(), AllocatorChannelServiceError> {
        self.flush_metadata()?;
        self.flush_pending_status()?;
        self.flush_pending_stream()?;

        if self.pending_status.is_some() || self.pending_stream.is_some() {
            return Ok(());
        }

        while let Some(request) = self.control_channel.try_receive(self.control_consumer)? {
            self.handle_request(request)?;
            self.flush_pending_status()?;
            self.flush_pending_stream()?;
            self.flush_metadata()?;

            if self.pending_status.is_some() || self.pending_stream.is_some() {
                break;
            }
        }

        Ok(())
    }

    fn flush_metadata(&mut self) -> Result<(), AllocatorChannelServiceError> {
        while self.next_metadata < self.domain_count {
            let domain = self.domain_ids[self.next_metadata];
            let info = self
                .allocator
                .domain(domain)
                .ok_or_else(AllocError::invalid_domain)?;
            match self.metadata_channel.try_send(
                self.metadata_producer,
                AllocatorDomainMetadataMessage::Advertised(info),
            ) {
                Ok(()) => self.next_metadata += 1,
                Err(error)
                    if matches!(
                        error.kind(),
                        ChannelErrorKind::Busy | ChannelErrorKind::ResourceExhausted
                    ) =>
                {
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn flush_pending_status(&mut self) -> Result<(), AllocatorChannelServiceError> {
        let Some(message) = self.pending_status else {
            return Ok(());
        };
        match self.status_channel.try_send(self.status_producer, message) {
            Ok(()) => {
                self.pending_status = None;
                Ok(())
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ChannelErrorKind::Busy | ChannelErrorKind::ResourceExhausted
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn handle_request(
        &mut self,
        request: AllocatorControlRequest,
    ) -> Result<(), AllocatorChannelServiceError> {
        match request {
            AllocatorControlRequest::ReadDomainAudit { domain } => {
                self.pending_status = Some(match self.allocator.domain_audit(domain) {
                    Ok(audit) => AllocatorControlStatusMessage::DomainAudit { domain, audit },
                    Err(error) => AllocatorControlStatusMessage::Rejected {
                        domain: Some(domain),
                        reason: error.kind,
                    },
                });
                Ok(())
            }
            AllocatorControlRequest::ReadDomainPoolStats { domain } => {
                self.pending_status = Some(match self.allocator.domain_pool_stats(domain) {
                    Ok(stats) => AllocatorControlStatusMessage::DomainPoolStats { domain, stats },
                    Err(error) => AllocatorControlStatusMessage::Rejected {
                        domain: Some(domain),
                        reason: error.kind,
                    },
                });
                Ok(())
            }
            AllocatorControlRequest::ReadDomainPoolMembers { domain } => {
                match self.allocator.domain(domain) {
                    Some(_) => {
                        self.pending_stream = Some(PendingStatusStream::PoolMembers {
                            domain,
                            next_index: 0,
                        });
                        Ok(())
                    }
                    None => {
                        self.pending_status = Some(AllocatorControlStatusMessage::Rejected {
                            domain: Some(domain),
                            reason: AllocErrorKind::InvalidDomain,
                        });
                        Ok(())
                    }
                }
            }
            AllocatorControlRequest::ReadDomainPoolExtents { domain } => {
                match self.allocator.domain(domain) {
                    Some(_) => {
                        self.pending_stream = Some(PendingStatusStream::PoolExtents {
                            domain,
                            next_index: 0,
                        });
                        Ok(())
                    }
                    None => {
                        self.pending_status = Some(AllocatorControlStatusMessage::Rejected {
                            domain: Some(domain),
                            reason: AllocErrorKind::InvalidDomain,
                        });
                        Ok(())
                    }
                }
            }
            AllocatorControlRequest::RepublishDomains => {
                self.next_metadata = 0;
                self.pending_status =
                    Some(AllocatorControlStatusMessage::MetadataRepublishScheduled);
                Ok(())
            }
        }
    }

    fn flush_pending_stream(&mut self) -> Result<(), AllocatorChannelServiceError> {
        loop {
            let Some(stream) = self.pending_stream else {
                return Ok(());
            };

            let message = match stream {
                PendingStatusStream::PoolMembers { domain, next_index } => {
                    match self
                        .allocator
                        .domain_pool_member_info_at(domain, next_index)
                    {
                        Ok(Some(member)) => {
                            self.pending_stream = Some(PendingStatusStream::PoolMembers {
                                domain,
                                next_index: next_index + 1,
                            });
                            AllocatorControlStatusMessage::DomainPoolMember { domain, member }
                        }
                        Ok(None) => {
                            self.pending_stream = None;
                            AllocatorControlStatusMessage::DomainPoolMembersComplete { domain }
                        }
                        Err(error) => {
                            self.pending_stream = None;
                            AllocatorControlStatusMessage::Rejected {
                                domain: Some(domain),
                                reason: error.kind,
                            }
                        }
                    }
                }
                PendingStatusStream::PoolExtents { domain, next_index } => {
                    match self
                        .allocator
                        .domain_pool_extent_info_at(domain, next_index)
                    {
                        Ok(Some(extent)) => {
                            self.pending_stream = Some(PendingStatusStream::PoolExtents {
                                domain,
                                next_index: next_index + 1,
                            });
                            AllocatorControlStatusMessage::DomainPoolExtent { domain, extent }
                        }
                        Ok(None) => {
                            self.pending_stream = None;
                            AllocatorControlStatusMessage::DomainPoolExtentsComplete { domain }
                        }
                        Err(error) => {
                            self.pending_stream = None;
                            AllocatorControlStatusMessage::Rejected {
                                domain: Some(domain),
                                reason: error.kind,
                            }
                        }
                    }
                }
            };

            match self.status_channel.try_send(self.status_producer, message) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        ChannelErrorKind::Busy | ChannelErrorKind::ResourceExhausted
                    ) =>
                {
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}
