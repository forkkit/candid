use super::lexer::error;
use super::typing::TypeEnv;
use crate::types::{Field, Label, Type};
use crate::{Error, Result};
use crate::{Int, Nat};
use serde::de;
use serde::de::{Deserialize, Visitor};
use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;

#[derive(Debug, PartialEq, Clone)]
pub enum IDLValue {
    Bool(bool),
    Null,
    Text(String),
    Number(String), // Undetermined number type
    Float64(f64),
    Opt(Box<IDLValue>),
    Vec(Vec<IDLValue>),
    Record(Vec<IDLField>),
    Variant(Box<IDLField>, u64), // u64 represents the index from the type, defaults to 0 when parsing
    Principal(crate::Principal),
    // The following values can only be generated with type annotation
    None,
    Int(Int),
    Nat(Nat),
    Nat8(u8),
    Nat16(u16),
    Nat32(u32),
    Nat64(u64),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    Float32(f32),
    Reserved,
}

#[derive(Debug, PartialEq, Clone)]
pub struct IDLField {
    pub id: Label,
    pub val: IDLValue,
}

#[derive(Debug, PartialEq, Clone)]
pub struct IDLArgs {
    pub args: Vec<IDLValue>,
}

impl IDLArgs {
    pub fn new(args: &[IDLValue]) -> Self {
        IDLArgs {
            args: args.to_owned(),
        }
    }
    pub fn annotate_types(self, from_parser: bool, env: &TypeEnv, types: &[Type]) -> Result<Self> {
        if types.len() > self.args.len() {
            return Err(Error::msg("wrong number of argument values"));
        }
        let mut args = Vec::new();
        for (v, ty) in self.args.iter().zip(types.iter()) {
            let v = v.annotate_type(from_parser, env, &ty)?;
            args.push(v);
        }
        Ok(IDLArgs { args })
    }
    /// Encode IDLArgs with the given types. Note that this is not equivalent to
    /// `idl_args.annotate_types(true, env, types).to_bytes()` for recursive types.
    pub fn to_bytes_with_types(&self, env: &TypeEnv, types: &[Type]) -> Result<Vec<u8>> {
        if types.len() > self.args.len() {
            return Err(Error::msg("wrong number of argument values"));
        }
        let mut idl = crate::ser::IDLBuilder::new();
        let empty_env = TypeEnv::new();
        for (i, (v, ty)) in self.args.iter().zip(types.iter()).enumerate() {
            if i == 0 {
                // env gets merged into the builder state, we only need to pass in env once.
                idl.value_arg_with_type(v, env, ty)?;
            } else {
                idl.value_arg_with_type(v, &empty_env, ty)?;
            }
        }
        idl.serialize_to_vec()
    }
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut idl = crate::ser::IDLBuilder::new();
        for v in self.args.iter() {
            idl.value_arg(v)?;
        }
        idl.serialize_to_vec()
    }
    pub fn from_bytes_with_types(bytes: &[u8], env: &TypeEnv, types: &[Type]) -> Result<Self> {
        let mut de = crate::de::IDLDeserialize::new(bytes)?;
        let mut args = Vec::new();
        for ty in types.iter() {
            let v = de.get_value::<IDLValue>()?;
            let v = v.annotate_type(false, env, ty)?;
            args.push(v);
        }
        de.done()?;
        Ok(IDLArgs { args })
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut de = crate::de::IDLDeserialize::new(bytes)?;
        let mut args = Vec::new();
        while !de.is_done() {
            let v = de.get_value::<IDLValue>()?;
            args.push(v);
        }
        de.done()?;
        Ok(IDLArgs { args })
    }
}

impl std::str::FromStr for IDLArgs {
    type Err = Error;
    fn from_str(str: &str) -> std::result::Result<Self, Self::Err> {
        let lexer = super::lexer::Lexer::new(str);
        Ok(super::grammar::ArgsParser::new().parse(lexer)?)
    }
}

impl std::str::FromStr for IDLValue {
    type Err = Error;
    fn from_str(str: &str) -> std::result::Result<Self, Self::Err> {
        let lexer = super::lexer::Lexer::new(str);
        Ok(super::grammar::ArgParser::new().parse(lexer)?)
    }
}

impl fmt::Display for IDLArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(")?;
        let len = self.args.len();
        for i in 0..len {
            write!(f, "{}", self.args[i])?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        write!(f, ")")
    }
}

impl fmt::Display for IDLValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            IDLValue::Null => write!(f, "null"),
            IDLValue::Bool(b) => write!(f, "{}", b),
            IDLValue::Number(ref str) => write!(f, "{}", str),
            IDLValue::Int(ref i) => write!(f, "{}", i),
            IDLValue::Nat(ref n) => write!(f, "{}", n),
            IDLValue::Nat8(n) => write!(f, "{}", n),
            IDLValue::Nat16(n) => write!(f, "{}", n),
            IDLValue::Nat32(n) => write!(f, "{}", n),
            IDLValue::Nat64(n) => write!(f, "{}", n),
            IDLValue::Int8(n) => write!(f, "{}", n),
            IDLValue::Int16(n) => write!(f, "{}", n),
            IDLValue::Int32(n) => write!(f, "{}", n),
            IDLValue::Int64(n) => write!(f, "{}", n),
            IDLValue::Float32(n) => write!(f, "{}", n),
            IDLValue::Float64(n) => write!(f, "{}", n),
            IDLValue::Text(ref s) => write!(f, "\"{}\"", s),
            IDLValue::None => write!(f, "null"),
            IDLValue::Reserved => write!(f, "reserved"),
            IDLValue::Opt(ref v) => write!(f, "opt {}", v),
            IDLValue::Vec(ref vec) => {
                write!(f, "vec {{ ")?;
                for e in vec.iter() {
                    write!(f, "{}; ", e)?;
                }
                write!(f, "}}")
            }
            IDLValue::Record(ref fs) => {
                write!(f, "record {{ ")?;
                for e in fs.iter() {
                    write!(f, "{}; ", e)?;
                }
                write!(f, "}}")
            }
            IDLValue::Variant(ref v, _) => write!(f, "variant {{ {} }}", v),
            IDLValue::Principal(ref id) => write!(f, "principal \"{}\"", id),
        }
    }
}

impl fmt::Display for IDLField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} = {}", self.id, self.val)
    }
}

impl IDLValue {
    /// Anotate `IDLValue` with the given type, allowing subtyping. If `IDLValue` is parser from
    /// string, we need to set `from_parser` to true to enable converting numbers to the expected types.
    pub fn annotate_type(&self, from_parser: bool, env: &TypeEnv, t: &Type) -> Result<Self> {
        Ok(match (self, t) {
            (_, Type::Var(id)) => {
                let ty = env.rec_find_type(id)?;
                self.annotate_type(from_parser, env, ty)?
            }
            (_, Type::Knot(id)) => {
                let ty = crate::types::internal::find_type(*id).unwrap();
                self.annotate_type(from_parser, env, &ty)?
            }
            (IDLValue::Null, Type::Opt(_)) if from_parser => IDLValue::None,
            (IDLValue::Float64(n), Type::Float32) if from_parser => IDLValue::Float32(*n as f32),
            (IDLValue::Number(str), t) if from_parser => match t {
                Type::Int => IDLValue::Int(str.parse::<Int>()?),
                Type::Nat => IDLValue::Nat(str.parse::<Nat>()?),
                Type::Nat8 => IDLValue::Nat8(str.parse::<u8>().map_err(error)?),
                Type::Nat16 => IDLValue::Nat16(str.parse::<u16>().map_err(error)?),
                Type::Nat32 => IDLValue::Nat32(str.parse::<u32>().map_err(error)?),
                Type::Nat64 => IDLValue::Nat64(str.parse::<u64>().map_err(error)?),
                Type::Int8 => IDLValue::Int8(str.parse::<i8>().map_err(error)?),
                Type::Int16 => IDLValue::Int16(str.parse::<i16>().map_err(error)?),
                Type::Int32 => IDLValue::Int32(str.parse::<i32>().map_err(error)?),
                Type::Int64 => IDLValue::Int64(str.parse::<i64>().map_err(error)?),
                _ => {
                    return Err(Error::msg(format!(
                        "type mismatch: {} can not be of type {}",
                        self, t
                    )))
                }
            },
            (_, Type::Reserved) => IDLValue::Reserved,
            (IDLValue::Null, Type::Null) => IDLValue::Null,
            (IDLValue::Bool(b), Type::Bool) => IDLValue::Bool(*b),
            (IDLValue::Int(i), Type::Int) => IDLValue::Int(i.clone()),
            (IDLValue::Nat(n), Type::Nat) => IDLValue::Nat(n.clone()),
            (IDLValue::Nat8(n), Type::Nat8) => IDLValue::Nat8(*n),
            (IDLValue::Nat16(n), Type::Nat16) => IDLValue::Nat16(*n),
            (IDLValue::Nat32(n), Type::Nat32) => IDLValue::Nat32(*n),
            (IDLValue::Nat64(n), Type::Nat64) => IDLValue::Nat64(*n),
            (IDLValue::Int8(n), Type::Int8) => IDLValue::Int8(*n),
            (IDLValue::Int16(n), Type::Int16) => IDLValue::Int16(*n),
            (IDLValue::Int32(n), Type::Int32) => IDLValue::Int32(*n),
            (IDLValue::Int64(n), Type::Int64) => IDLValue::Int64(*n),
            (IDLValue::Float64(n), Type::Float64) => IDLValue::Float64(*n),
            (IDLValue::Float32(n), Type::Float32) => IDLValue::Float32(*n),
            (IDLValue::Text(s), Type::Text) => IDLValue::Text(s.to_owned()),
            (IDLValue::None, Type::Opt(_)) => IDLValue::None,
            (IDLValue::Opt(v), Type::Opt(ty)) => {
                let v = v.annotate_type(from_parser, env, ty)?;
                IDLValue::Opt(Box::new(v))
            }
            (IDLValue::Vec(vec), Type::Vec(ty)) => {
                let mut res = Vec::new();
                for e in vec.iter() {
                    let v = e.annotate_type(from_parser, env, ty)?;
                    res.push(v);
                }
                IDLValue::Vec(res)
            }
            (IDLValue::Record(vec), Type::Record(fs)) => {
                let fields: HashMap<_, _> =
                    vec.iter().map(|IDLField { id, val }| (id, val)).collect();
                let mut res = Vec::new();
                for Field { id, ty } in fs.iter() {
                    let val = fields
                        .get(&id)
                        .ok_or_else(|| Error::msg(format!("field {} not found", id)))?;
                    let val = val.annotate_type(from_parser, env, ty)?;
                    res.push(IDLField {
                        id: id.clone(),
                        val,
                    });
                }
                IDLValue::Record(res)
            }
            (IDLValue::Variant(v, _), Type::Variant(fs)) => {
                for (i, f) in fs.iter().enumerate() {
                    if v.id == f.id {
                        let val = v.val.annotate_type(from_parser, env, &f.ty)?;
                        let field = IDLField {
                            id: f.id.clone(),
                            val,
                        };
                        return Ok(IDLValue::Variant(Box::new(field), i as u64));
                    }
                }
                return Err(Error::msg(format!("field {} not found", v.id)));
            }
            (IDLValue::Principal(id), Type::Principal) => IDLValue::Principal(id.clone()),
            _ => {
                return Err(Error::msg(format!(
                    "type mismatch: {} cannot be of type {}",
                    self, t
                )))
            }
        })
    }
    // This will only be called when the type is not provided
    pub fn value_ty(&self) -> Type {
        match *self {
            IDLValue::Null => Type::Null,
            IDLValue::Bool(_) => Type::Bool,
            IDLValue::Number(_) => Type::Int, // Number defaults to Int
            IDLValue::Int(_) => Type::Int,
            IDLValue::Nat(_) => Type::Nat,
            IDLValue::Nat8(_) => Type::Nat8,
            IDLValue::Nat16(_) => Type::Nat16,
            IDLValue::Nat32(_) => Type::Nat32,
            IDLValue::Nat64(_) => Type::Nat64,
            IDLValue::Int8(_) => Type::Int8,
            IDLValue::Int16(_) => Type::Int16,
            IDLValue::Int32(_) => Type::Int32,
            IDLValue::Int64(_) => Type::Int64,
            IDLValue::Float32(_) => Type::Float32,
            IDLValue::Float64(_) => Type::Float64,
            IDLValue::Text(_) => Type::Text,
            IDLValue::None => Type::Opt(Box::new(Type::Null)),
            IDLValue::Reserved => Type::Reserved,
            IDLValue::Opt(ref v) => {
                let t = v.deref().value_ty();
                Type::Opt(Box::new(t))
            }
            IDLValue::Vec(ref vec) => {
                let t = if vec.is_empty() {
                    Type::Null
                } else {
                    vec[0].value_ty()
                };
                Type::Vec(Box::new(t))
            }
            IDLValue::Record(ref vec) => {
                let fs: Vec<_> = vec
                    .iter()
                    .map(|IDLField { id, val }| Field {
                        id: id.clone(),
                        ty: val.value_ty(),
                    })
                    .collect();
                Type::Record(fs)
            }
            IDLValue::Variant(ref v, idx) => {
                assert_eq!(idx, 0);
                let f = Field {
                    id: v.id.clone(),
                    ty: v.val.value_ty(),
                };
                Type::Variant(vec![f])
            }
            IDLValue::Principal(_) => Type::Principal,
        }
    }
}

pub mod pretty {
    use super::*;
    use crate::pretty::*;

    use ::pretty::RcDoc;

    pub use crate::bindings::candid::pp_label;

    fn pp_field(field: &IDLField) -> RcDoc {
        pp_label(&field.id)
            .append(kwd(" ="))
            .append(pp_value(&field.val))
    }

    fn pp_fields(fields: &[IDLField]) -> RcDoc {
        concat(fields.iter().map(|f| pp_field(f)), ";")
    }

    pub fn pp_value(v: &IDLValue) -> RcDoc {
        use super::IDLValue::*;
        match &*v {
            Opt(v) => kwd("opt").append(enclose_space("{", pp_value(v), "}")),
            Vec(vs) => {
                let body = concat(vs.iter().map(|v| pp_value(v)), ";");
                kwd("vec").append(enclose_space("{", body, "}"))
            }
            Record(fields) => kwd("record").append(enclose_space("{", pp_fields(&fields), "}")),
            Variant(v, _) => kwd("variant").append(enclose_space("{", pp_field(&v), "}")),
            _ => RcDoc::as_string(v),
        }
    }

    pub fn pp_args(args: &IDLArgs) -> RcDoc {
        let body = concat(args.args.iter().map(pp_value), ",");
        enclose("(", body, ")")
    }
}

impl crate::CandidType for IDLValue {
    fn ty() -> Type {
        unreachable!();
    }
    fn id() -> crate::types::TypeId {
        unreachable!();
    }
    fn _ty() -> Type {
        unreachable!();
    }
    fn idl_serialize<S>(&self, serializer: S) -> std::result::Result<(), S::Error>
    where
        S: crate::types::Serializer,
    {
        use crate::types::Compound;
        match *self {
            IDLValue::Null => serializer.serialize_null(()),
            IDLValue::Bool(b) => serializer.serialize_bool(b),
            IDLValue::Number(ref str) => {
                let v = str.parse::<Int>().map_err(serde::ser::Error::custom)?;
                serializer.serialize_int(&v)
            }
            IDLValue::Int(ref i) => serializer.serialize_int(i),
            IDLValue::Nat(ref n) => serializer.serialize_nat(n),
            IDLValue::Nat8(n) => serializer.serialize_nat8(n),
            IDLValue::Nat16(n) => serializer.serialize_nat16(n),
            IDLValue::Nat32(n) => serializer.serialize_nat32(n),
            IDLValue::Nat64(n) => serializer.serialize_nat64(n),
            IDLValue::Int8(n) => serializer.serialize_int8(n),
            IDLValue::Int16(n) => serializer.serialize_int16(n),
            IDLValue::Int32(n) => serializer.serialize_int32(n),
            IDLValue::Int64(n) => serializer.serialize_int64(n),
            IDLValue::Float32(f) => serializer.serialize_float32(f),
            IDLValue::Float64(f) => serializer.serialize_float64(f),
            IDLValue::Text(ref s) => serializer.serialize_text(s),
            IDLValue::None => serializer.serialize_option::<Option<String>>(None),
            IDLValue::Opt(ref v) => serializer.serialize_option(Some(v.deref())),
            IDLValue::Vec(ref vec) => {
                let mut ser = serializer.serialize_vec(vec.len())?;
                for e in vec.iter() {
                    ser.serialize_element(&e)?;
                }
                Ok(())
            }
            IDLValue::Record(ref vec) => {
                let mut ser = serializer.serialize_struct()?;
                for f in vec.iter() {
                    ser.serialize_element(&f.val)?;
                }
                Ok(())
            }
            IDLValue::Variant(ref v, idx) => {
                let mut ser = serializer.serialize_variant(idx)?;
                ser.serialize_element(&v.val)?;
                Ok(())
            }
            IDLValue::Principal(ref id) => serializer.serialize_principal(&id.0),
            IDLValue::Reserved => serializer.serialize_null(()),
        }
    }
}

type DResult<E> = std::result::Result<IDLValue, E>;

macro_rules! visit_prim {
    ($name:ident, $ty:ty) => {
        paste::item! {
            fn [<visit_ $ty>]<E>(self, value: $ty) -> DResult<E> {
                Ok(IDLValue::$name(value))
            }
        }
    };
}

impl<'de> Deserialize<'de> for IDLValue {
    fn deserialize<D>(deserializer: D) -> DResult<D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct IDLValueVisitor;

        impl<'de> Visitor<'de> for IDLValueVisitor {
            type Value = IDLValue;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("any valid Candid value")
            }
            visit_prim!(Bool, bool);
            visit_prim!(Nat8, u8);
            visit_prim!(Nat16, u16);
            visit_prim!(Nat32, u32);
            visit_prim!(Nat64, u64);
            visit_prim!(Int8, i8);
            visit_prim!(Int16, i16);
            visit_prim!(Int32, i32);
            visit_prim!(Int64, i64);
            visit_prim!(Float32, f32);
            visit_prim!(Float64, f64);
            // Deserialize Candid specific types: Bignumber, principal, reversed
            fn visit_bytes<E: de::Error>(self, value: &[u8]) -> DResult<E> {
                let (tag, bytes) = value.split_at(1);
                match tag[0] {
                    0u8 => {
                        let v = Int(num_bigint::BigInt::from_signed_bytes_le(bytes));
                        Ok(IDLValue::Int(v))
                    }
                    1u8 => {
                        let v = Nat(num_bigint::BigUint::from_bytes_le(bytes));
                        Ok(IDLValue::Nat(v))
                    }
                    2u8 => {
                        let v = crate::Principal::from_bytes(bytes);
                        Ok(IDLValue::Principal(v))
                    }
                    3u8 => Ok(IDLValue::Reserved),
                    _ => Err(de::Error::custom("unknown tag in visit_bytes")),
                }
            }
            fn visit_string<E>(self, value: String) -> DResult<E> {
                Ok(IDLValue::Text(value))
            }
            fn visit_str<E>(self, value: &str) -> DResult<E>
            where
                E: serde::de::Error,
            {
                self.visit_string(String::from(value))
            }
            fn visit_none<E>(self) -> DResult<E> {
                Ok(IDLValue::None)
            }
            fn visit_some<D>(self, deserializer: D) -> DResult<D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let v = Deserialize::deserialize(deserializer)?;
                Ok(IDLValue::Opt(Box::new(v)))
            }
            fn visit_unit<E>(self) -> DResult<E> {
                Ok(IDLValue::Null)
            }
            fn visit_seq<V>(self, mut visitor: V) -> DResult<V::Error>
            where
                V: de::SeqAccess<'de>,
            {
                let mut vec = Vec::new();
                while let Some(elem) = visitor.next_element()? {
                    vec.push(elem);
                }
                Ok(IDLValue::Vec(vec))
            }
            fn visit_map<V>(self, mut visitor: V) -> DResult<V::Error>
            where
                V: de::MapAccess<'de>,
            {
                let mut vec = Vec::new();
                while let Some((key, value)) = visitor.next_entry()? {
                    if let IDLValue::Nat32(hash) = key {
                        let f = IDLField {
                            id: Label::Id(hash),
                            val: value,
                        };
                        vec.push(f);
                    } else {
                        unreachable!()
                    }
                }
                Ok(IDLValue::Record(vec))
            }
            fn visit_enum<V>(self, data: V) -> DResult<V::Error>
            where
                V: de::EnumAccess<'de>,
            {
                use serde::de::VariantAccess;
                let (variant, visitor) = data.variant::<IDLValue>()?;
                if let IDLValue::Text(v) = variant {
                    let v: Vec<_> = v.split(',').collect();
                    assert_eq!(v.len(), 2);
                    let id = v[0].parse::<u32>().unwrap();
                    let val = match v[1] {
                        "unit" => {
                            visitor.unit_variant()?;
                            IDLValue::Null
                        }
                        "struct" => visitor.struct_variant(&[], self)?,
                        "newtype" => visitor.newtype_variant()?,
                        _ => unreachable!(),
                    };
                    let f = IDLField {
                        id: Label::Id(id),
                        val,
                    };
                    // Deserialized variant always has 0 index to ensure untyped
                    // serialization is correct.
                    Ok(IDLValue::Variant(Box::new(f), 0))
                } else {
                    unreachable!()
                }
            }
        }

        deserializer.deserialize_any(IDLValueVisitor)
    }
}
