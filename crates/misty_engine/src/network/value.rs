//! Values sent over the network (RPC arguments, Network Variables) and the little-endian byte
//! writer / reader every message uses.

use glam::{Quat, Vec3};

/// A value a script can send: what Lua values become on the wire.
#[derive(Clone, Debug, PartialEq)]
pub enum NetValue {
    Nil,
    Bool(bool),
    Int(i64),
    Number(f64),
    Text(String),
    Vector3(Vec3),
    /// A spawned network object, by network id (a GameObject argument).
    Object(u64),
    /// A Lua table: key/value pairs in a stable order (array part first).
    Table(Vec<(NetValue, NetValue)>),
}

/// Deepest table nesting accepted (a cycle or a runaway structure stops there).
pub const MAX_DEPTH: usize = 16;

impl NetValue {
    pub fn write(&self, w: &mut Writer) {
        self.write_depth(w, 0);
    }

    fn write_depth(&self, w: &mut Writer, depth: usize) {
        match self {
            NetValue::Nil => w.u8(0),
            NetValue::Bool(false) => w.u8(1),
            NetValue::Bool(true) => w.u8(2),
            NetValue::Int(i) => {
                w.u8(3);
                w.varint(zigzag(*i));
            }
            NetValue::Number(n) => {
                w.u8(4);
                w.f64(*n);
            }
            NetValue::Text(s) => {
                w.u8(5);
                w.str(s);
            }
            NetValue::Vector3(v) => {
                w.u8(6);
                w.vec3(*v);
            }
            NetValue::Object(id) => {
                w.u8(7);
                w.varint(*id);
            }
            NetValue::Table(pairs) => {
                w.u8(8);
                if depth >= MAX_DEPTH {
                    w.varint(0);
                    return;
                }
                w.varint(pairs.len() as u64);
                for (k, v) in pairs {
                    k.write_depth(w, depth + 1);
                    v.write_depth(w, depth + 1);
                }
            }
        }
    }

    pub fn read(r: &mut Reader) -> Option<NetValue> {
        Self::read_depth(r, 0)
    }

    fn read_depth(r: &mut Reader, depth: usize) -> Option<NetValue> {
        Some(match r.u8()? {
            0 => NetValue::Nil,
            1 => NetValue::Bool(false),
            2 => NetValue::Bool(true),
            3 => NetValue::Int(unzigzag(r.varint()?)),
            4 => NetValue::Number(r.f64()?),
            5 => NetValue::Text(r.str()?),
            6 => NetValue::Vector3(r.vec3()?),
            7 => NetValue::Object(r.varint()?),
            8 => {
                if depth >= MAX_DEPTH {
                    return None;
                }
                let n = r.varint()? as usize;
                // Each pair takes at least two bytes: refuse lengths the packet can't hold.
                if n > r.remaining() / 2 {
                    return None;
                }
                let mut pairs = Vec::with_capacity(n);
                for _ in 0..n {
                    let k = Self::read_depth(r, depth + 1)?;
                    let v = Self::read_depth(r, depth + 1)?;
                    pairs.push((k, v));
                }
                NetValue::Table(pairs)
            }
            _ => return None,
        })
    }

    /// Short description for warnings ("number", "table", …).
    pub fn type_name(&self) -> &'static str {
        match self {
            NetValue::Nil => "nil",
            NetValue::Bool(_) => "boolean",
            NetValue::Int(_) | NetValue::Number(_) => "number",
            NetValue::Text(_) => "string",
            NetValue::Vector3(_) => "Vector3",
            NetValue::Object(_) => "GameObject",
            NetValue::Table(_) => "table",
        }
    }
}

fn zigzag(i: i64) -> u64 {
    ((i << 1) ^ (i >> 63)) as u64
}

fn unzigzag(u: u64) -> i64 {
    ((u >> 1) as i64) ^ -((u & 1) as i64)
}

/// Appends little-endian values to a byte buffer.
#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    /// LEB128: small numbers (ids, counts) take one byte.
    pub fn varint(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }
    pub fn str(&mut self, s: &str) {
        self.varint(s.len() as u64);
        self.buf.extend_from_slice(s.as_bytes());
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.varint(b.len() as u64);
        self.buf.extend_from_slice(b);
    }
    pub fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    pub fn quat(&mut self, q: Quat) {
        self.f32(q.x);
        self.f32(q.y);
        self.f32(q.z);
        self.f32(q.w);
    }
}

/// Reads what a [`Writer`] wrote; every read returns `None` past the end (a truncated or
/// garbled packet is dropped, never a panic).
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    pub fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
    }
    pub fn u64(&mut self) -> Option<u64> {
        self.take(8)
            .and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes)
    }
    pub fn f32(&mut self) -> Option<f32> {
        self.take(4)
            .and_then(|b| b.try_into().ok())
            .map(f32::from_le_bytes)
    }
    pub fn f64(&mut self) -> Option<f64> {
        self.take(8)
            .and_then(|b| b.try_into().ok())
            .map(f64::from_le_bytes)
    }
    pub fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    pub fn str(&mut self) -> Option<String> {
        let n = self.varint()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).ok()
    }
    pub fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.varint()? as usize;
        self.take(n).map(<[u8]>::to_vec)
    }
    pub fn vec3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    pub fn quat(&mut self) -> Option<Quat> {
        let q = Quat::from_xyzw(self.f32()?, self.f32()?, self.f32()?, self.f32()?);
        // Garbage or zero quaternions would poison transforms.
        (q.is_finite() && q.length_squared() > 1e-6).then(|| q.normalize())
    }
    /// The bytes not read yet.
    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.data[self.pos..];
        self.pos = self.data.len();
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_garbage_is_refused() {
        let v = NetValue::Table(vec![
            (NetValue::Int(1), NetValue::Text("héllo".into())),
            (NetValue::Int(2), NetValue::Number(-2.5)),
            (
                NetValue::Text("pos".into()),
                NetValue::Vector3(Vec3::new(1.0, 2.0, 3.0)),
            ),
            (NetValue::Text("ok".into()), NetValue::Bool(true)),
            (NetValue::Text("who".into()), NetValue::Object(77)),
            (NetValue::Text("neg".into()), NetValue::Int(-300)),
            (NetValue::Text("none".into()), NetValue::Nil),
        ]);
        let mut w = Writer::new();
        v.write(&mut w);
        let mut r = Reader::new(&w.buf);
        assert_eq!(NetValue::read(&mut r), Some(v));
        assert!(r.is_empty());
        // Truncated data and unknown tags read as None instead of panicking.
        for cut in 0..w.buf.len() {
            let _ = NetValue::read(&mut Reader::new(&w.buf[..cut]));
        }
        assert_eq!(NetValue::read(&mut Reader::new(&[99])), None);
        // A huge claimed table length is refused before allocating.
        assert_eq!(
            NetValue::read(&mut Reader::new(&[8, 0xff, 0xff, 0xff, 0x0f])),
            None
        );
        let mut w = Writer::new();
        w.varint(300);
        w.varint(u64::MAX);
        let mut r = Reader::new(&w.buf);
        assert_eq!((r.varint(), r.varint()), (Some(300), Some(u64::MAX)));
    }
}
