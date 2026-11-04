//! Asset GUIDs: 128-bit random ids written as 32 lowercase hex digits.

use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Stable id of an asset, kept in its `.meta` file. Zero is never handed out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Guid(pub u128);

impl Guid {
    /// A fresh random GUID (std's randomly keyed hasher over a clock, a counter and the pid).
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let half = |salt: u64| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(salt);
            h.write_u64(n);
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            h.finish()
        };
        let g = (u128::from(half(1)) << 64) | u128::from(half(2));
        Self(g.max(1))
    }
}

impl Default for Guid {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl FromStr for Guid {
    type Err = String;

    /// Exactly 32 hex digits (either case), not all zero.
    fn from_str(s: &str) -> Result<Self, String> {
        if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("'{s}' is not a GUID"));
        }
        match u128::from_str_radix(s, 16) {
            Ok(0) | Err(_) => Err(format!("'{s}' is not a GUID")),
            Ok(v) => Ok(Self(v)),
        }
    }
}
