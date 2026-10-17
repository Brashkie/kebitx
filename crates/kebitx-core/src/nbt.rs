//! Little-endian NBT, as Bedrock stores it in `level.dat`.
//!
//! `level.dat` = 8-byte header (u32 storage version, u32 payload length) then
//! one named compound tag. The bytes are untrusted like every other input:
//! every length is checked against the bytes that remain BEFORE anything is
//! allocated, nesting is bounded, and a malformed file is an `Err`.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Nbt>),
    Compound(BTreeMap<String, Nbt>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Nbt {
    pub fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(m) => m.get(key),
            _ => None,
        }
    }
    /// Any integer tag, widened. Flags in `level.dat` are bytes.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Nbt::Byte(v) => Some(v.into()),
            Nbt::Short(v) => Some(v.into()),
            Nbt::Int(v) => Some(v.into()),
            Nbt::Long(v) => Some(v),
            _ => None,
        }
    }
}

const MAX_DEPTH: usize = 64;

/// Parse a whole `level.dat`: header, then the root compound.
pub fn parse_level_dat(bytes: &[u8]) -> Result<Nbt, &'static str> {
    let body = bytes.get(8..).ok_or("truncated level.dat header")?;
    let mut r = Reader { b: body, i: 0 };
    let tag = r.u8()?;
    if tag != 10 {
        return Err("level.dat root is not a compound");
    }
    r.string()?; // root name, empty in practice
    r.payload(10, 0)
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        let end = self.i.checked_add(n).ok_or("length overflow")?;
        let s = self.b.get(self.i..end).ok_or("truncated NBT")?;
        self.i = end;
        Ok(s)
    }
    fn remaining(&self) -> usize {
        self.b.len().saturating_sub(self.i)
    }
    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], &'static str> {
        let mut a = [0u8; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }
    /// A count, refused if `count * elem` bytes cannot possibly remain: a
    /// hostile file must not make us allocate gigabytes up front.
    fn count(&mut self, elem: usize) -> Result<usize, &'static str> {
        let n = i32::from_le_bytes(self.arr()?);
        let n = usize::try_from(n).map_err(|_| "negative length")?;
        if n.checked_mul(elem)
            .is_none_or(|bytes| bytes > self.remaining())
        {
            return Err("length larger than the file");
        }
        Ok(n)
    }
    fn string(&mut self) -> Result<String, &'static str> {
        let n = u16::from_le_bytes(self.arr()?) as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn payload(&mut self, tag: u8, depth: usize) -> Result<Nbt, &'static str> {
        if depth > MAX_DEPTH {
            return Err("NBT nested too deep");
        }
        Ok(match tag {
            1 => Nbt::Byte(i8::from_le_bytes(self.arr()?)),
            2 => Nbt::Short(i16::from_le_bytes(self.arr()?)),
            3 => Nbt::Int(i32::from_le_bytes(self.arr()?)),
            4 => Nbt::Long(i64::from_le_bytes(self.arr()?)),
            5 => Nbt::Float(f32::from_le_bytes(self.arr()?)),
            6 => Nbt::Double(f64::from_le_bytes(self.arr()?)),
            7 => {
                let n = self.count(1)?;
                Nbt::ByteArray(self.take(n)?.to_vec())
            }
            8 => Nbt::String(self.string()?),
            9 => {
                let elem = self.u8()?;
                // every element takes at least one byte, except in an empty list
                let n = self.count(if elem == 0 { 0 } else { 1 })?;
                let mut v = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    v.push(self.payload(elem, depth + 1)?);
                }
                Nbt::List(v)
            }
            10 => {
                let mut m = BTreeMap::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let name = self.string()?;
                    m.insert(name, self.payload(t, depth + 1)?);
                }
                Nbt::Compound(m)
            }
            11 => {
                let n = self.count(4)?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(i32::from_le_bytes(self.arr()?));
                }
                Nbt::IntArray(v)
            }
            12 => {
                let n = self.count(8)?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(i64::from_le_bytes(self.arr()?));
                }
                Nbt::LongArray(v)
            }
            0 if depth > 0 => return Err("end tag outside a compound"),
            _ => return Err("unknown NBT tag"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// header + root compound { experiments: { gametest: 1b }, LevelName: "w" }
    fn sample() -> Vec<u8> {
        let mut body = vec![10, 0, 0];
        body.extend([10]);
        body.extend(11u16.to_le_bytes());
        body.extend(b"experiments");
        body.extend([1]);
        body.extend(8u16.to_le_bytes());
        body.extend(b"gametest");
        body.extend([1, 0]); // value 1, end of experiments
        body.extend([8]);
        body.extend(9u16.to_le_bytes());
        body.extend(b"LevelName");
        body.extend(1u16.to_le_bytes());
        body.extend(b"w");
        body.extend([0]); // end of root
        let mut b = 10u32.to_le_bytes().to_vec();
        b.extend((body.len() as u32).to_le_bytes());
        b.extend(body);
        b
    }

    #[test]
    fn reads_experiments() {
        let root = parse_level_dat(&sample()).unwrap();
        let flag = root
            .get("experiments")
            .and_then(|e| e.get("gametest"))
            .and_then(Nbt::as_i64);
        assert_eq!(flag, Some(1));
        assert_eq!(root.get("LevelName"), Some(&Nbt::String("w".into())));
    }

    #[test]
    fn hostile_input_is_an_error_not_a_crash() {
        let full = sample();
        for cut in 0..full.len() {
            let _ = parse_level_dat(&full[..cut]);
        }
        // a list claiming two billion elements in a few bytes
        let mut b = vec![0u8; 8];
        b.extend([10, 0, 0, 9, 1, 0, b'x', 1]);
        b.extend(i32::MAX.to_le_bytes());
        assert!(parse_level_dat(&b).is_err());
        // nesting far past the limit
        let mut deep = vec![0u8; 8];
        deep.extend([10, 0, 0]);
        for _ in 0..1000 {
            deep.extend([10, 1, 0, b'a']);
        }
        assert!(parse_level_dat(&deep).is_err());
    }
}
