//! Reading ZIP archives (a Java `.jar`, a `.mcpack`): the central directory
//! is read once, an entry's data on demand. Stored and DEFLATE entries; each
//! one's CRC-32 is checked. No ZIP64, no encryption: refused, not guessed.

use super::{crc32, inflate};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
struct EntryInfo {
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    local_offset: u64,
}

/// Where the archive's bytes are: a file, read on demand, or memory (a
/// `.jar` inside a `.jar`, never written to disk).
enum Backing {
    File(PathBuf),
    Memory(Arc<[u8]>),
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

pub struct ZipArchive {
    backing: Backing,
    entries: HashMap<String, EntryInfo>,
}

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl ZipArchive {
    pub fn open(path: &Path) -> Result<Self, String> {
        let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        Self::directory(f, len, Backing::File(path.to_path_buf()))
    }

    /// An archive held in memory: a `.jar` bundled inside another one.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let data: Arc<[u8]> = bytes.into();
        let len = data.len() as u64;
        Self::directory(Cursor::new(data.clone()), len, Backing::Memory(data))
    }

    fn reader(&self) -> Result<Box<dyn ReadSeek + '_>, String> {
        Ok(match &self.backing {
            Backing::File(p) => Box::new(File::open(p).map_err(|e| e.to_string())?),
            Backing::Memory(d) => Box::new(Cursor::new(&d[..])),
        })
    }

    fn directory<R: Read + Seek>(mut f: R, len: u64, backing: Backing) -> Result<Self, String> {
        // the end record is in the last 22 + 65535 bytes (comment included)
        let tail_len = len.min(22 + 65535);
        f.seek(SeekFrom::Start(len - tail_len))
            .map_err(|e| e.to_string())?;
        let mut tail = vec![0u8; tail_len as usize];
        f.read_exact(&mut tail).map_err(|e| e.to_string())?;
        let eocd = (0..tail.len().saturating_sub(21))
            .rev()
            .find(|&i| le32(&tail, i) == 0x0605_4b50)
            .ok_or("not a ZIP archive (no end record)")?;
        let count = le16(&tail, eocd + 10) as usize;
        let cd_size = le32(&tail, eocd + 12) as u64;
        let cd_offset = le32(&tail, eocd + 16) as u64;
        if cd_offset == 0xffff_ffff || count == 0xffff {
            return Err("ZIP64 archives are not supported".into());
        }
        if cd_offset.checked_add(cd_size).is_none_or(|e| e > len) {
            return Err("ZIP central directory out of the file".into());
        }
        f.seek(SeekFrom::Start(cd_offset))
            .map_err(|e| e.to_string())?;
        let mut cd = vec![0u8; cd_size as usize];
        f.read_exact(&mut cd).map_err(|e| e.to_string())?;
        let mut entries = HashMap::with_capacity(count);
        let mut i = 0;
        for _ in 0..count {
            let head = cd.get(i..i + 46).ok_or("ZIP central directory cut short")?;
            if le32(head, 0) != 0x0201_4b50 {
                return Err("bad ZIP central directory entry".into());
            }
            let flags = le16(head, 8);
            let (name_len, extra_len, comment_len) = (
                le16(head, 28) as usize,
                le16(head, 30) as usize,
                le16(head, 32) as usize,
            );
            let name = cd
                .get(i + 46..i + 46 + name_len)
                .ok_or("ZIP name cut short")?;
            let info = EntryInfo {
                method: le16(head, 10),
                crc: le32(head, 16),
                compressed: le32(head, 20) as u64,
                size: le32(head, 24) as u64,
                local_offset: le32(head, 42) as u64,
            };
            if flags & 1 == 0 {
                entries.insert(String::from_utf8_lossy(name).replace('\\', "/"), info);
            }
            i += 46 + name_len + extra_len + comment_len;
        }
        Ok(ZipArchive { backing, entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// An entry's bytes, if it exists and is at most `limit` bytes.
    pub fn read(&self, name: &str, limit: u64) -> Result<Vec<u8>, String> {
        let e = self
            .entries
            .get(name)
            .ok_or_else(|| format!("{name}: not in the archive"))?;
        if e.size > limit {
            return Err(format!("{name}: larger than allowed"));
        }
        let mut f = self.reader()?;
        f.seek(SeekFrom::Start(e.local_offset))
            .map_err(|e| e.to_string())?;
        let mut head = [0u8; 30];
        f.read_exact(&mut head).map_err(|e| e.to_string())?;
        if le32(&head, 0) != 0x0403_4b50 {
            return Err(format!("{name}: bad local header"));
        }
        let skip = le16(&head, 26) as i64 + le16(&head, 28) as i64;
        f.seek(SeekFrom::Current(skip)).map_err(|e| e.to_string())?;
        let mut packed = vec![
            0u8;
            e.compressed
                .min(limit.saturating_mul(1032).saturating_add(1024))
                as usize
        ];
        f.read_exact(&mut packed)
            .map_err(|err| format!("{name}: {err}"))?;
        let data = match e.method {
            0 => packed,
            8 => {
                inflate::inflate(&packed, e.size as usize)
                    .map_err(|err| format!("{name}: {err}"))?
                    .0
            }
            m => return Err(format!("{name}: compression method {m} not supported")),
        };
        if data.len() as u64 != e.size || crc32(&data) != e.crc {
            return Err(format!("{name}: size or CRC mismatch"));
        }
        Ok(data)
    }
}
