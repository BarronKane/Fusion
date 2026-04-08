//! Canonical qualified courier names and Fusion surface locators.

use core::fmt;

/// Fixed-capacity parse/format error for courier-qualified names and Fusion locators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FusionLocatorError {
    Invalid,
    ResourceExhausted,
}

impl FusionLocatorError {
    #[must_use]
    pub const fn invalid() -> Self {
        Self::Invalid
    }

    #[must_use]
    pub const fn resource_exhausted() -> Self {
        Self::ResourceExhausted
    }
}

/// Returns whether one courier-local name is valid for the canonical qualified grammar.
///
/// Courier-local names intentionally reject delimiters used by the qualified-name grammar so the
/// parser never has to guess whether one byte belongs to the local name or to the scope syntax.
#[must_use]
pub fn is_valid_courier_local_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.bytes().all(|byte| {
        !matches!(byte, b'.' | b'@' | b'[' | b']' | b'/')
            && !byte.is_ascii_control()
            && !byte.is_ascii_whitespace()
    })
}

/// One fixed-capacity, nearest-first chain of context-root courier names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextChain<'a, const MAX_DEPTH: usize> {
    depth: usize,
    names: [Option<&'a str>; MAX_DEPTH],
}

impl<'a, const MAX_DEPTH: usize> ContextChain<'a, MAX_DEPTH> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            depth: 0,
            names: [None; MAX_DEPTH],
        }
    }

    pub fn push(&mut self, name: &'a str) -> Result<(), FusionLocatorError> {
        if !is_valid_courier_local_name(name) {
            return Err(FusionLocatorError::invalid());
        }
        if self.depth >= MAX_DEPTH {
            return Err(FusionLocatorError::resource_exhausted());
        }
        self.names[self.depth] = Some(name);
        self.depth += 1;
        Ok(())
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.depth
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.depth == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.names[..self.depth].iter().flatten().copied()
    }
}

impl<'a, const MAX_DEPTH: usize> Default for ContextChain<'a, MAX_DEPTH> {
    fn default() -> Self {
        Self::new()
    }
}

/// Canonical typed identity for one courier scoped within visible context roots on one domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QualifiedCourierName<'a, const MAX_CHAIN: usize> {
    courier: &'a str,
    context_chain: ContextChain<'a, MAX_CHAIN>,
    domain: &'a str,
}

impl<'a, const MAX_CHAIN: usize> QualifiedCourierName<'a, MAX_CHAIN> {
    pub fn new(courier: &'a str, domain: &'a str) -> Result<Self, FusionLocatorError> {
        if !is_valid_courier_local_name(courier) || domain.is_empty() {
            return Err(FusionLocatorError::invalid());
        }
        Ok(Self {
            courier,
            context_chain: ContextChain::new(),
            domain,
        })
    }

    pub fn parse(input: &'a str) -> Result<Self, FusionLocatorError> {
        let Some(domain_start) = input.rfind('[') else {
            return Err(FusionLocatorError::invalid());
        };
        if !input.ends_with(']') || domain_start == 0 {
            return Err(FusionLocatorError::invalid());
        }
        let authority = &input[..domain_start];
        let domain = &input[(domain_start + 1)..(input.len() - 1)];
        let (courier, contexts) = match authority.split_once('@') {
            Some((courier, contexts)) => {
                if courier.is_empty() || contexts.is_empty() {
                    return Err(FusionLocatorError::invalid());
                }
                (courier, Some(contexts))
            }
            None => (authority, None),
        };
        let mut qualified = Self::new(courier, domain)?;
        if let Some(contexts) = contexts {
            for context in contexts.split('.') {
                qualified.push_context_root(context)?;
            }
        }
        Ok(qualified)
    }

    pub fn push_context_root(&mut self, name: &'a str) -> Result<(), FusionLocatorError> {
        self.context_chain.push(name)
    }

    #[must_use]
    pub const fn courier(&self) -> &'a str {
        self.courier
    }

    #[must_use]
    pub const fn domain(&self) -> &'a str {
        self.domain
    }

    #[must_use]
    pub const fn context_chain(&self) -> &ContextChain<'a, MAX_CHAIN> {
        &self.context_chain
    }
}

impl<const MAX_CHAIN: usize> fmt::Display for QualifiedCourierName<'_, MAX_CHAIN> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.courier)?;
        if !self.context_chain.is_empty() {
            write!(f, "@")?;
            let mut first = true;
            for context in self.context_chain.iter() {
                if !first {
                    write!(f, ".")?;
                }
                first = false;
                write!(f, "{context}")?;
            }
        }
        write!(f, "[{}]", self.domain)
    }
}

/// Surface kind for one canonical Fusion locator path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FusionSurfaceKind<'a> {
    Channel,
    Service,
    Device,
    Custom(&'a str),
}

impl<'a> FusionSurfaceKind<'a> {
    pub fn parse(input: &'a str) -> Result<Self, FusionLocatorError> {
        if input.is_empty() || input.contains('/') {
            return Err(FusionLocatorError::invalid());
        }
        Ok(match input {
            "channel" => Self::Channel,
            "service" => Self::Service,
            "device" => Self::Device,
            other => Self::Custom(other),
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'a str {
        match self {
            Self::Channel => "channel",
            Self::Service => "service",
            Self::Device => "device",
            Self::Custom(kind) => kind,
        }
    }
}

/// Canonical Fusion URI over one qualified courier identity and one named surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FusionSurfaceRef<'a, const MAX_CHAIN: usize> {
    courier: QualifiedCourierName<'a, MAX_CHAIN>,
    kind: FusionSurfaceKind<'a>,
    name: &'a str,
}

impl<'a, const MAX_CHAIN: usize> FusionSurfaceRef<'a, MAX_CHAIN> {
    pub fn new(
        courier: QualifiedCourierName<'a, MAX_CHAIN>,
        kind: FusionSurfaceKind<'a>,
        name: &'a str,
    ) -> Result<Self, FusionLocatorError> {
        if name.is_empty() || name.contains('/') {
            return Err(FusionLocatorError::invalid());
        }
        Ok(Self {
            courier,
            kind,
            name,
        })
    }

    pub fn parse(input: &'a str) -> Result<Self, FusionLocatorError> {
        let Some(rest) = input.strip_prefix("fusion://") else {
            return Err(FusionLocatorError::invalid());
        };
        let Some((authority, path)) = rest.split_once('/') else {
            return Err(FusionLocatorError::invalid());
        };
        let Some((kind, name)) = path.split_once('/') else {
            return Err(FusionLocatorError::invalid());
        };
        if name.contains('/') {
            return Err(FusionLocatorError::invalid());
        }
        Self::new(
            QualifiedCourierName::parse(authority)?,
            FusionSurfaceKind::parse(kind)?,
            name,
        )
    }

    #[must_use]
    pub const fn courier(&self) -> QualifiedCourierName<'a, MAX_CHAIN> {
        self.courier
    }

    #[must_use]
    pub const fn kind(&self) -> FusionSurfaceKind<'a> {
        self.kind
    }

    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }
}

impl<const MAX_CHAIN: usize> fmt::Display for FusionSurfaceRef<'_, MAX_CHAIN> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "fusion://{}/{}/{}",
            self.courier,
            self.kind.as_str(),
            self.name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_courier_name_round_trips_with_context_chain() {
        let name = QualifiedCourierName::<4>::parse("cyw43439@firmware.root-courier[fusion.local]")
            .expect("qualified courier name should parse");
        assert_eq!(name.courier(), "cyw43439");
        assert_eq!(name.domain(), "fusion.local");
        assert_eq!(
            name.context_chain().iter().collect::<Vec<_>>(),
            vec!["firmware", "root-courier"]
        );
        assert_eq!(
            name.to_string(),
            "cyw43439@firmware.root-courier[fusion.local]"
        );
    }

    #[test]
    fn qualified_courier_name_without_context_chain_round_trips() {
        let name =
            QualifiedCourierName::<4>::parse("root-courier[fusion.local]").expect("name parses");
        assert_eq!(name.courier(), "root-courier");
        assert!(name.context_chain().is_empty());
        assert_eq!(name.to_string(), "root-courier[fusion.local]");
    }

    #[test]
    fn invalid_courier_local_names_are_rejected() {
        assert!(!is_valid_courier_local_name("firmware.kernel"));
        assert!(!is_valid_courier_local_name("firmware@kernel"));
        assert!(!is_valid_courier_local_name("firmware[kernel]"));
        assert!(!is_valid_courier_local_name("firmware/service"));
        assert!(is_valid_courier_local_name("shell#02"));
    }

    #[test]
    fn fusion_surface_ref_round_trips() {
        let surface = FusionSurfaceRef::<4>::parse(
            "fusion://shell#02@walance.users.root-courier[pvas.me]/channel/stdin",
        )
        .expect("surface ref should parse");
        assert_eq!(surface.kind(), FusionSurfaceKind::Channel);
        assert_eq!(surface.name(), "stdin");
        assert_eq!(
            surface.to_string(),
            "fusion://shell#02@walance.users.root-courier[pvas.me]/channel/stdin"
        );
    }
}
