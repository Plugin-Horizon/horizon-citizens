//! A reader for the part of Java object serialization that Citizens' item blobs use: the
//! `rO0AB...` base64 streams of `saves.yml` (Bukkit's `BukkitObjectOutputStream` around an
//! `ItemMeta` map). It reads objects, class descriptors with their superclasses, references,
//! strings, arrays, enums and the custom `writeObject` data of the collections it meets
//! (`HashMap`, `ArrayList`, Guava's `ImmutableMap` form). Everything else is an error, so an
//! unknown stream is never half-read as data.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Str(String),
    Int(i32),
    Long(i64),
    /// Primitive fields other than `int` and `long`, as their raw bytes.
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Object(Object),
}

/// One object: its class and the superclass chain's field values (superclass first), plus the
/// `writeObject` data the classes that have it wrote after their fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub class: String,
    /// The classes of the chain, subclass first.
    pub chain: Vec<String>,
    pub fields: Vec<(String, Value)>,
    pub data: Vec<Annotation>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Annotation {
    /// Block data (`writeInt` and the like).
    Bytes(Vec<u8>),
    Value(Value),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug)]
enum Handle {
    Pending,
    Class(ClassDesc),
    Value(Value),
}

#[derive(Clone, Debug)]
struct ClassDesc {
    name: String,
    flags: u8,
    fields: Vec<(u8, String)>,
    sup: Option<Box<ClassDesc>>,
}

const BASE_HANDLE: u32 = 0x7e_0000;

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
    handles: Vec<Handle>,
}

fn err<T>(msg: &str) -> Result<T, Error> {
    Err(Error(msg.to_owned()))
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self.i.checked_add(n).filter(|e| *e <= self.b.len());
        let Some(end) = end else {
            return err("stream ends early");
        };
        let s = &self.b[self.i..end];
        self.i = end;
        Ok(s)
    }
    fn u1(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn u2(&mut self) -> Result<u16, Error> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn i4(&mut self) -> Result<i32, Error> {
        let s = self.take(4)?;
        Ok(i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i8(&mut self) -> Result<i64, Error> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(i64::from_be_bytes(a))
    }
    fn utf(&mut self) -> Result<String, Error> {
        let n = self.u2()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn new_handle(&mut self) -> usize {
        self.handles.push(Handle::Pending);
        self.handles.len() - 1
    }
    fn handle_index(&mut self) -> Result<usize, Error> {
        let raw = self.i4()? as u32;
        let idx = raw.checked_sub(BASE_HANDLE).ok_or_else(|| Error("bad reference".into()))? as usize;
        if idx >= self.handles.len() {
            return err("reference to an unknown object");
        }
        Ok(idx)
    }

    /// A class descriptor or `null`/a reference to one.
    fn class_desc(&mut self) -> Result<Option<ClassDesc>, Error> {
        match self.u1()? {
            0x70 => Ok(None),
            0x71 => {
                let idx = self.handle_index()?;
                match self.handles[idx].clone() {
                    Handle::Class(c) => Ok(Some(c)),
                    _ => err("reference is not a class"),
                }
            }
            0x72 => {
                let name = self.utf()?;
                let _suid = self.i8()?;
                let idx = self.new_handle();
                let flags = self.u1()?;
                let n = self.u2()?;
                let mut fields = Vec::new();
                for _ in 0..n {
                    let code = self.u1()?;
                    let fname = self.utf()?;
                    if code == b'L' || code == b'[' {
                        self.read_value()?; // the field's class name
                    }
                    fields.push((code, fname));
                }
                self.skip_annotation()?;
                let sup = self.class_desc()?.map(Box::new);
                let cd = ClassDesc {
                    name,
                    flags,
                    fields,
                    sup,
                };
                self.handles[idx] = Handle::Class(cd.clone());
                Ok(Some(cd))
            }
            t => Err(Error(format!("class descriptor tag {t:#x} expected"))),
        }
    }

    fn skip_annotation(&mut self) -> Result<(), Error> {
        loop {
            match self.b.get(self.i).copied() {
                Some(0x78) => {
                    self.i += 1;
                    return Ok(());
                }
                Some(_) => {
                    self.read_content()?;
                }
                None => return err("stream ends early"),
            }
        }
    }

    /// One block-data or object item of a `writeObject` annotation.
    fn read_content(&mut self) -> Result<Annotation, Error> {
        match self.b.get(self.i).copied() {
            Some(0x77) => {
                self.i += 1;
                let n = self.u1()? as usize;
                Ok(Annotation::Bytes(self.take(n)?.to_vec()))
            }
            Some(0x7a) => {
                self.i += 1;
                let n = self.i4()? as usize;
                Ok(Annotation::Bytes(self.take(n)?.to_vec()))
            }
            _ => Ok(Annotation::Value(self.read_value()?)),
        }
    }

    fn read_value(&mut self) -> Result<Value, Error> {
        match self.u1()? {
            0x70 => Ok(Value::Null),
            0x71 => {
                let idx = self.handle_index()?;
                match self.handles[idx].clone() {
                    Handle::Value(v) => Ok(v),
                    Handle::Pending => err("reference to an unfinished object"),
                    Handle::Class(_) => err("reference is a class"),
                }
            }
            0x74 => {
                let n = self.u2()? as usize;
                let s = String::from_utf8_lossy(self.take(n)?).into_owned();
                self.handles.push(Handle::Value(Value::Str(s.clone())));
                Ok(Value::Str(s))
            }
            0x7c => {
                let n = self.i8()? as usize;
                let s = String::from_utf8_lossy(self.take(n)?).into_owned();
                self.handles.push(Handle::Value(Value::Str(s.clone())));
                Ok(Value::Str(s))
            }
            0x75 => {
                let cd = self.class_desc()?.ok_or_else(|| Error("array without class".into()))?;
                let idx = self.new_handle();
                let n = self.i4()?;
                if n < 0 {
                    return err("negative array length");
                }
                let elem = cd.name.as_bytes().get(1).copied().unwrap_or(b'L');
                let mut items = Vec::new();
                for _ in 0..n {
                    let v = match elem {
                        b'I' => Value::Int(self.i4()?),
                        b'L' | b'[' => self.read_value()?,
                        b'B' | b'Z' => Value::Bytes(self.take(1)?.to_vec()),
                        b'C' | b'S' => Value::Bytes(self.take(2)?.to_vec()),
                        b'F' => Value::Bytes(self.take(4)?.to_vec()),
                        b'D' | b'J' => Value::Bytes(self.take(8)?.to_vec()),
                        _ => return err("unknown array element type"),
                    };
                    items.push(v);
                }
                let v = Value::Array(items);
                self.handles[idx] = Handle::Value(v.clone());
                Ok(v)
            }
            0x73 => {
                let cd = self.class_desc()?.ok_or_else(|| Error("object without class".into()))?;
                let idx = self.new_handle();
                let mut chain = Vec::new();
                let mut c = Some(&cd);
                while let Some(x) = c {
                    chain.push(x.name.clone());
                    c = x.sup.as_deref();
                }
                // Superclass data comes first.
                let mut descs = Vec::new();
                let mut c = Some(cd.clone());
                while let Some(x) = c {
                    c = x.sup.as_deref().cloned();
                    descs.push(x);
                }
                let mut fields = Vec::new();
                let mut data = Vec::new();
                for d in descs.iter().rev() {
                    for (code, fname) in &d.fields {
                        let v = match code {
                            b'I' => Value::Int(self.i4()?),
                            b'J' => Value::Long(self.i8()?),
                            b'L' | b'[' => self.read_value()?,
                            b'B' | b'Z' => Value::Bytes(self.take(1)?.to_vec()),
                            b'C' | b'S' => Value::Bytes(self.take(2)?.to_vec()),
                            b'F' => Value::Bytes(self.take(4)?.to_vec()),
                            b'D' => Value::Bytes(self.take(8)?.to_vec()),
                            _ => return err("unknown field type"),
                        };
                        fields.push((fname.clone(), v));
                    }
                    if d.flags & 0x01 != 0 {
                        // SC_WRITE_METHOD: the class wrote more after its fields.
                        loop {
                            if self.b.get(self.i).copied() == Some(0x78) {
                                self.i += 1;
                                break;
                            }
                            data.push(self.read_content()?);
                        }
                    } else if d.flags & 0x04 != 0 {
                        return err("externalizable objects are not read");
                    }
                }
                let v = Value::Object(Object {
                    class: cd.name.clone(),
                    chain,
                    fields,
                    data,
                });
                self.handles[idx] = Handle::Value(v.clone());
                Ok(v)
            }
            0x7e => {
                // An enum constant: its name, as a string.
                let _ = self.class_desc()?;
                let idx = self.new_handle();
                let name = self.read_value()?;
                self.handles[idx] = Handle::Value(name.clone());
                Ok(name)
            }
            t => Err(Error(format!("unsupported object tag {t:#x}"))),
        }
    }
}

/// Reads a whole stream (`AC ED 00 05` header, one top-level object).
pub fn read(bytes: &[u8]) -> Result<Value, Error> {
    if bytes.len() < 4 || bytes[0..2] != [0xac, 0xed] || bytes[2..4] != [0x00, 0x05] {
        return err("not a Java serialization stream");
    }
    let mut r = Reader {
        b: bytes,
        i: 4,
        handles: Vec::new(),
    };
    let v = r.read_value()?;
    if r.i != bytes.len() {
        return err("bytes after the top-level object");
    }
    Ok(v)
}

impl Object {
    pub fn field(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    pub fn is_a(&self, class: &str) -> bool {
        self.chain.iter().any(|c| c == class)
    }
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
    /// The entries of a Guava immutable map (`ImmutableMap$SerializedForm`, also under a bi-map)
    /// or a `java.util.HashMap`.
    pub fn map_entries(&self) -> Option<Vec<(Value, Value)>> {
        let o = self.as_object()?;
        if o.is_a("com.google.common.collect.ImmutableMap$SerializedForm") {
            let keys = match o.field("keys")? {
                Value::Array(a) => a,
                _ => return None,
            };
            let values = match o.field("values")? {
                Value::Array(a) => a,
                _ => return None,
            };
            return Some(keys.iter().cloned().zip(values.iter().cloned()).collect());
        }
        if o.class == "java.util.HashMap" {
            // The data: buckets and size (8 bytes), then key and value objects.
            let mut items = o.data.iter();
            match items.next()? {
                Annotation::Bytes(b) if b.len() == 8 => {}
                _ => return None,
            }
            let rest: Vec<Value> = items
                .filter_map(|a| match a {
                    Annotation::Value(v) => Some(v.clone()),
                    Annotation::Bytes(_) => None,
                })
                .collect();
            return Some(rest.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].clone(), c[1].clone())).collect());
        }
        None
    }
    /// The elements of a `java.util.ArrayList`.
    pub fn list_items(&self) -> Option<Vec<Value>> {
        let o = self.as_object()?;
        if o.class != "java.util.ArrayList" {
            return None;
        }
        Some(
            o.data
                .iter()
                .filter_map(|a| match a {
                    Annotation::Value(v) => Some(v.clone()),
                    Annotation::Bytes(_) => None,
                })
                .collect(),
        )
    }
    /// The value of a wrapped number (`java.lang.Integer`, `Long` and the like with a `value`).
    pub fn boxed_int(&self) -> Option<i32> {
        let o = self.as_object()?;
        if o.class != "java.lang.Integer" {
            return None;
        }
        o.field("value")?.as_int()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::{base64_decode, fixtures::POTION};

    #[test]
    fn reads_the_bukkit_wrapper_and_its_map() {
        let bytes = base64_decode(POTION).unwrap();
        let v = read(&bytes).unwrap();
        let wrapper = v.as_object().unwrap();
        assert_eq!(wrapper.class, "org.bukkit.util.io.Wrapper");
        let entries = wrapper.field("map").unwrap().map_entries().unwrap();
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str().unwrap()).collect();
        assert_eq!(keys, ["==", "meta-type", "potion-type"]);
        assert_eq!(entries[2].1.as_str(), Some("minecraft:strong_swiftness"));
    }

    #[test]
    fn integers_in_a_map_are_boxed_objects() {
        let bytes = base64_decode(crate::items::fixtures::BLADE).unwrap();
        let v = read(&bytes).unwrap();
        let entries = v.as_object().unwrap().field("map").unwrap().map_entries().unwrap();
        let (_, enchants) = entries.iter().find(|(k, _)| k.as_str() == Some("enchants")).unwrap();
        let levels = enchants.map_entries().unwrap();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].1.boxed_int(), Some(5));
        assert_eq!(levels[1].1.boxed_int(), Some(3));
    }

    #[test]
    fn a_bad_header_or_a_cut_stream_is_refused() {
        assert!(read(&[0x00, 0x01, 0x02, 0x03]).is_err());
        let bytes = base64_decode(POTION).unwrap();
        assert!(read(&bytes[..bytes.len() - 3]).is_err());
        let mut long = bytes.clone();
        long.push(0);
        assert!(read(&long).is_err(), "trailing bytes are an error");
    }

    #[test]
    fn a_class_with_write_data_keeps_its_block_data() {
        // A HashMap written by Java: buckets and size as block data, then the entries.
        let mut map = Vec::new();
        map.extend_from_slice(&[0xac, 0xed, 0x00, 0x05]);
        map.push(0x73);
        map.push(0x72);
        map.extend_from_slice(&[0x00, 0x11]);
        map.extend_from_slice(b"java.util.HashMap");
        map.extend_from_slice(&[0; 8]);
        map.push(0x03); // flags: SC_SERIALIZABLE | SC_WRITE_METHOD
        map.extend_from_slice(&[0x00, 0x02]);
        map.push(b'F');
        map.extend_from_slice(&[0x00, 0x0a]);
        map.extend_from_slice(b"loadFactor");
        map.push(b'I');
        map.extend_from_slice(&[0x00, 0x09]);
        map.extend_from_slice(b"threshold");
        map.push(0x78);
        map.push(0x70);
        map.extend_from_slice(&0.75f32.to_be_bytes());
        map.extend_from_slice(&12i32.to_be_bytes());
        map.push(0x77);
        map.push(8);
        map.extend_from_slice(&16i32.to_be_bytes());
        map.extend_from_slice(&1i32.to_be_bytes());
        map.push(0x74);
        map.extend_from_slice(&[0x00, 0x01]);
        map.extend_from_slice(b"k");
        map.push(0x74);
        map.extend_from_slice(&[0x00, 0x01]);
        map.extend_from_slice(b"v");
        map.push(0x78);
        let v = read(&map).unwrap();
        let entries = v.map_entries().unwrap();
        assert_eq!(entries, vec![(Value::Str("k".into()), Value::Str("v".into()))]);
    }
}
