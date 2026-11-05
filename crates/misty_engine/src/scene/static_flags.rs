//! Static Editor Flags: which baked systems treat a Static object as static (the dropdown next
//! to the Inspector's Static checkbox). A flag only counts while the Static checkbox is on, and
//! every flag is set by default, so ticking Static alone keeps its old meaning.

use serde::{Deserialize, Serialize};

/// Bit set of static uses. Saved as a number, left out of files while it is [`ALL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StaticFlags(pub u32);

impl StaticFlags {
    pub const CONTRIBUTE_GI: StaticFlags = StaticFlags(1);
    pub const OCCLUDER: StaticFlags = StaticFlags(1 << 1);
    pub const BATCHING: StaticFlags = StaticFlags(1 << 2);
    pub const NAVIGATION: StaticFlags = StaticFlags(1 << 3);
    pub const OCCLUDEE: StaticFlags = StaticFlags(1 << 4);
    pub const OFF_MESH_LINK: StaticFlags = StaticFlags(1 << 5);
    pub const REFLECTION_PROBE: StaticFlags = StaticFlags(1 << 6);
    pub const NOTHING: StaticFlags = StaticFlags(0);
    pub const ALL: StaticFlags = StaticFlags((1 << 7) - 1);

    /// Dropdown entries in menu order.
    pub const LIST: [(StaticFlags, &'static str); 7] = [
        (Self::CONTRIBUTE_GI, "Contribute GI"),
        (Self::OCCLUDER, "Occluder Static"),
        (Self::BATCHING, "Batching Static"),
        (Self::NAVIGATION, "Navigation Static"),
        (Self::OCCLUDEE, "Occludee Static"),
        (Self::OFF_MESH_LINK, "Off Mesh Link Generation"),
        (Self::REFLECTION_PROBE, "Reflection Probe Static"),
    ];

    pub fn contains(self, f: StaticFlags) -> bool {
        self.0 & f.0 == f.0
    }

    pub fn set(&mut self, f: StaticFlags, on: bool) {
        if on {
            self.0 |= f.0;
        } else {
            self.0 &= !f.0;
        }
    }

    pub fn is_all(&self) -> bool {
        *self == Self::ALL
    }
}

impl std::ops::BitOr for StaticFlags {
    type Output = StaticFlags;
    fn bitor(self, o: StaticFlags) -> StaticFlags {
        StaticFlags(self.0 | o.0)
    }
}

impl Default for StaticFlags {
    fn default() -> Self {
        Self::ALL
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::GameObject;

    #[test]
    fn flags_default_to_everything_and_stay_out_of_files() {
        let go = GameObject::default();
        assert!(go.static_flags.is_all());
        let json = serde_json::to_string(&go).unwrap();
        assert!(!json.contains("static_flags"), "{json}");
        let mut go = go;
        go.is_static = true;
        assert!(go.has_static(StaticFlags::OCCLUDER));
        go.static_flags.set(StaticFlags::OCCLUDER, false);
        assert!(!go.has_static(StaticFlags::OCCLUDER));
        assert!(go.has_static(StaticFlags::OCCLUDEE));
        let back: GameObject = serde_json::from_str(&serde_json::to_string(&go).unwrap()).unwrap();
        assert_eq!(back.static_flags, go.static_flags);
        go.is_static = false;
        assert!(!go.has_static(StaticFlags::OCCLUDEE), "needs Static ticked");
    }
}
