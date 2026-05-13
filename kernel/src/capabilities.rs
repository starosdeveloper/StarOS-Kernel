//! IPC Capabilities and Access Control
//!
//! Implements capability-based security for inter-process communication.
//! Each process holds a set of capabilities that determine what IPC
//! operations it can perform.

use crate::error::KernelError;
use crate::process::task::TaskId;

/// Capability rights bitfield
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rights(u32);

impl Rights {
    pub const NONE: Self = Self(0);
    pub const SEND: Self = Self(1 << 0);
    pub const RECEIVE: Self = Self(1 << 1);
    pub const GRANT: Self = Self(1 << 2);
    pub const MAP: Self = Self(1 << 3);
    pub const SIGNAL: Self = Self(1 << 4);
    pub const CREATE_CHILD: Self = Self(1 << 5);
    pub const DESTROY: Self = Self(1 << 6);
    pub const ALL: Self = Self(0x7F);

    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn union(&self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn intersect(&self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Remove rights (can only reduce, never add)
    pub const fn restrict(&self, mask: Self) -> Self {
        Self(self.0 & mask.0)
    }
}

/// A capability token - unforgeable reference to a kernel object
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    /// Object ID this capability refers to
    pub object_id: u64,
    /// Rights granted by this capability
    pub rights: Rights,
    /// Owner task
    pub owner: TaskId,
    /// Generation counter (invalidated on revoke)
    pub generation: u32,
}

/// Capability space for a single task (max 64 capabilities)
pub const MAX_CAPS_PER_TASK: usize = 64;

pub struct CapabilitySpace {
    caps: [Option<Capability>; MAX_CAPS_PER_TASK],
    owner: TaskId,
}

impl CapabilitySpace {
    pub fn new(owner: TaskId) -> Self {
        Self {
            caps: [None; MAX_CAPS_PER_TASK],
            owner,
        }
    }

    /// Insert a capability, returns slot index
    pub fn insert(&mut self, cap: Capability) -> Result<usize, KernelError> {
        if cap.owner != self.owner {
            return Err(KernelError::Security(crate::error::SecurityError::AccessDenied));
        }

        for (i, slot) in self.caps.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(cap);
                return Ok(i);
            }
        }
        Err(KernelError::ResourceExhausted)
    }

    /// Lookup capability by slot
    pub fn get(&self, slot: usize) -> Option<&Capability> {
        self.caps.get(slot)?.as_ref()
    }

    /// Remove capability from slot
    pub fn remove(&mut self, slot: usize) -> Option<Capability> {
        self.caps.get_mut(slot)?.take()
    }

    /// Check if task has a capability with given rights for an object
    pub fn has_rights(&self, object_id: u64, required: Rights) -> bool {
        self.caps.iter().any(|slot| {
            slot.as_ref().map_or(false, |cap| {
                cap.object_id == object_id && cap.rights.contains(required)
            })
        })
    }

    /// Grant a capability to another task (requires GRANT right)
    pub fn grant(
        &self,
        slot: usize,
        target_owner: TaskId,
        restricted_rights: Rights,
    ) -> Result<Capability, KernelError> {
        let cap = self.get(slot)
            .ok_or(KernelError::NotFound)?;

        if !cap.rights.contains(Rights::GRANT) {
            return Err(KernelError::Security(crate::error::SecurityError::PermissionDenied));
        }

        // New capability has at most the intersection of original rights and restriction
        let new_rights = cap.rights.intersect(restricted_rights);

        Ok(Capability {
            object_id: cap.object_id,
            rights: new_rights,
            owner: target_owner,
            generation: cap.generation,
        })
    }

    /// Count active capabilities
    pub fn count(&self) -> usize {
        self.caps.iter().filter(|s| s.is_some()).count()
    }
}

/// Validate IPC send permission
pub fn check_send_permission(
    sender_caps: &CapabilitySpace,
    channel_id: u64,
) -> Result<(), KernelError> {
    if sender_caps.has_rights(channel_id, Rights::SEND) {
        Ok(())
    } else {
        Err(KernelError::Security(crate::error::SecurityError::PermissionDenied))
    }
}

/// Validate IPC receive permission
pub fn check_receive_permission(
    receiver_caps: &CapabilitySpace,
    channel_id: u64,
) -> Result<(), KernelError> {
    if receiver_caps.has_rights(channel_id, Rights::RECEIVE) {
        Ok(())
    } else {
        Err(KernelError::Security(crate::error::SecurityError::PermissionDenied))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rights() {
        let r = Rights::SEND.union(Rights::RECEIVE);
        assert!(r.contains(Rights::SEND));
        assert!(r.contains(Rights::RECEIVE));
        assert!(!r.contains(Rights::GRANT));
    }

    #[test]
    fn test_capability_space() {
        let owner = TaskId::new(1);
        let mut space = CapabilitySpace::new(owner);

        let cap = Capability {
            object_id: 42,
            rights: Rights::SEND.union(Rights::RECEIVE),
            owner,
            generation: 0,
        };

        let slot = space.insert(cap).unwrap();
        assert_eq!(space.count(), 1);
        assert!(space.has_rights(42, Rights::SEND));
        assert!(!space.has_rights(42, Rights::GRANT));

        space.remove(slot);
        assert_eq!(space.count(), 0);
    }

    #[test]
    fn test_grant() {
        let owner = TaskId::new(1);
        let target = TaskId::new(2);
        let mut space = CapabilitySpace::new(owner);

        let cap = Capability {
            object_id: 100,
            rights: Rights::ALL,
            owner,
            generation: 0,
        };

        let slot = space.insert(cap).unwrap();
        let granted = space.grant(slot, target, Rights::SEND).unwrap();

        assert_eq!(granted.owner, target);
        assert!(granted.rights.contains(Rights::SEND));
        assert!(!granted.rights.contains(Rights::DESTROY));
    }

    #[test]
    fn test_grant_without_permission() {
        let owner = TaskId::new(1);
        let target = TaskId::new(2);
        let mut space = CapabilitySpace::new(owner);

        let cap = Capability {
            object_id: 100,
            rights: Rights::SEND, // No GRANT right
            owner,
            generation: 0,
        };

        let slot = space.insert(cap).unwrap();
        assert!(space.grant(slot, target, Rights::SEND).is_err());
    }

    #[test]
    fn test_check_permissions() {
        let owner = TaskId::new(1);
        let mut space = CapabilitySpace::new(owner);

        let cap = Capability {
            object_id: 50,
            rights: Rights::SEND,
            owner,
            generation: 0,
        };
        space.insert(cap).unwrap();

        assert!(check_send_permission(&space, 50).is_ok());
        assert!(check_receive_permission(&space, 50).is_err());
    }
}
