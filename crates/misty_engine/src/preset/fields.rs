//! Every field name of a serialisable struct, including the ones its serializer leaves out at
//! their defaults (`skip_serializing_if`): a tiny `Serializer` that records the keys the derived
//! `Serialize` reports through `serialize_field` and `skip_field`, without serialising values.

use std::fmt;

use serde::Serialize;
use serde::ser::{self, Impossible, SerializeStruct, Serializer};

/// Field names of `value` in declaration order; an empty list when it isn't a plain struct.
pub fn names<T: Serialize>(value: &T) -> Vec<String> {
    let mut names = Names(Vec::new());
    match value.serialize(&mut names) {
        Ok(()) => names.0,
        Err(_) => Vec::new(),
    }
}

/// Anything but a struct stops the walk.
#[derive(Debug)]
struct Stop;

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a struct")
    }
}

impl std::error::Error for Stop {}

impl ser::Error for Stop {
    fn custom<T: fmt::Display>(_: T) -> Self {
        Stop
    }
}

struct Names(Vec<String>);

impl SerializeStruct for &mut Names {
    type Ok = ();
    type Error = Stop;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        _: &T,
    ) -> Result<(), Stop> {
        self.0.push(key.to_owned());
        Ok(())
    }

    fn skip_field(&mut self, key: &'static str) -> Result<(), Stop> {
        self.0.push(key.to_owned());
        Ok(())
    }

    fn end(self) -> Result<(), Stop> {
        Ok(())
    }
}

/// Declares the scalar `serialize_*` methods as failures.
macro_rules! stop {
    ($($f:ident($t:ty)),*) => {
        $(fn $f(self, _: $t) -> Result<(), Stop> { Err(Stop) })*
    };
}

impl Serializer for &mut Names {
    type Ok = ();
    type Error = Stop;
    type SerializeSeq = Impossible<(), Stop>;
    type SerializeTuple = Impossible<(), Stop>;
    type SerializeTupleStruct = Impossible<(), Stop>;
    type SerializeTupleVariant = Impossible<(), Stop>;
    type SerializeMap = Impossible<(), Stop>;
    type SerializeStruct = Self;
    type SerializeStructVariant = Impossible<(), Stop>;

    stop!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8]),
        serialize_unit_struct(&'static str)
    );

    fn serialize_none(self) -> Result<(), Stop> {
        Err(Stop)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<(), Stop> {
        Err(Stop)
    }
    fn serialize_unit(self) -> Result<(), Stop> {
        Err(Stop)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<(), Stop> {
        Err(Stop)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<(), Stop> {
        Err(Stop)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<(), Stop> {
        Err(Stop)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Stop> {
        Err(Stop)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Stop> {
        Err(Stop)
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Stop> {
        Err(Stop)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Stop> {
        Err(Stop)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Stop> {
        Err(Stop)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Stop> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Stop> {
        Err(Stop)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lists_fields_left_out_at_their_defaults() {
        let light = crate::scene::Light::default();
        let names = super::names(&light);
        let saved = serde_json::to_value(&light).unwrap();
        assert!(!saved.as_object().unwrap().contains_key("halo"));
        assert!(names.iter().any(|n| n == "halo"));
        assert_eq!(names[0], "enabled");
        assert!(super::names(&3.0f32).is_empty());
    }
}
