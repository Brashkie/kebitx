//! A minimal ZIP writer for `.mcpack` / `.mcaddon`: std only.
//!
//! Entries are STORED (no compression): every ZIP reader, the game's included,
//! accepts it, and most of an add-on's weight (PNG, OGG) is compressed already.
//! Every entry gets the same fixed date, so the same input gives the same
//! bytes — reproducible builds. No ZIP64: an archive past 4 GiB or 65535
//! entries is an error, not a corrupt file.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

const DOS_DATE: u16 = (1 << 5) | 1; // 1980-01-01
const DOS_TIME: u16 = 0;
const UTF8_NAMES: u16 = 1 << 11;

/// Write `entries` (archive name, file on disk) to `out`. Names use `/`.
pub fn write_zip(out: &Path, entries: &[(String, std::path::PathBuf)]) -> Result<(), String> {
    if entries.len() >= u16::MAX as usize {
        return Err(format!(
            "{} entries: more than a ZIP without ZIP64 holds",
            entries.len()
        ));
    }
    let file = File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut w = BufWriter::new(file);
    let mut central = Vec::new();
    let mut offset: u64 = 0;

    for (name, path) in entries {
        let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let size = u32::try_from(data.len())
            .map_err(|_| format!("{}: larger than 4 GiB", path.display()))?;
        let at = u32::try_from(offset).map_err(|_| "archive larger than 4 GiB".to_string())?;
        let crc = crc32(&data);
        let name_len = u16::try_from(name.len()).map_err(|_| format!("name too long: {name}"))?;

        let mut local = Vec::with_capacity(30 + name.len());
        local.extend(0x0403_4b50u32.to_le_bytes());
        local.extend(20u16.to_le_bytes()); // version needed
        local.extend(UTF8_NAMES.to_le_bytes());
        local.extend(0u16.to_le_bytes()); // stored
        local.extend(DOS_TIME.to_le_bytes());
        local.extend(DOS_DATE.to_le_bytes());
        local.extend(crc.to_le_bytes());
        local.extend(size.to_le_bytes());
        local.extend(size.to_le_bytes());
        local.extend(name_len.to_le_bytes());
        local.extend(0u16.to_le_bytes()); // extra
        local.extend(name.as_bytes());
        w.write_all(&local).map_err(|e| e.to_string())?;
        w.write_all(&data).map_err(|e| e.to_string())?;
        offset += local.len() as u64 + data.len() as u64;

        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend(20u16.to_le_bytes()); // made by
        central.extend(20u16.to_le_bytes()); // needed
        central.extend(UTF8_NAMES.to_le_bytes());
        central.extend(0u16.to_le_bytes());
        central.extend(DOS_TIME.to_le_bytes());
        central.extend(DOS_DATE.to_le_bytes());
        central.extend(crc.to_le_bytes());
        central.extend(size.to_le_bytes());
        central.extend(size.to_le_bytes());
        central.extend(name_len.to_le_bytes());
        central.extend([0u8; 12]); // extra, comment, disk, internal attr, external attr
        central.extend(at.to_le_bytes());
        central.extend(name.as_bytes());
    }

    let cd_offset = u32::try_from(offset).map_err(|_| "archive larger than 4 GiB".to_string())?;
    let cd_size =
        u32::try_from(central.len()).map_err(|_| "central directory too large".to_string())?;
    let count = entries.len() as u16;
    w.write_all(&central).map_err(|e| e.to_string())?;
    let mut end = Vec::with_capacity(22);
    end.extend(0x0605_4b50u32.to_le_bytes());
    end.extend([0u8; 4]); // disk numbers
    end.extend(count.to_le_bytes());
    end.extend(count.to_le_bytes());
    end.extend(cd_size.to_le_bytes());
    end.extend(cd_offset.to_le_bytes());
    end.extend(0u16.to_le_bytes()); // comment
    w.write_all(&end).map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

/// CRC-32 (IEEE), as ZIP requires: the core's.
pub fn crc32(data: &[u8]) -> u32 {
    kebitx_core::codec::crc32(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_values() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn archive_layout() {
        let dir = std::env::temp_dir().join(format!("kebitx-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), "hello").unwrap();
        let out = dir.join("x.zip");
        write_zip(&out, &[("BP/a.txt".into(), dir.join("a.txt"))]).unwrap();
        let z = fs::read(&out).unwrap();
        assert_eq!(&z[..4], &0x0403_4b50u32.to_le_bytes());
        assert_eq!(&z[30..38], b"BP/a.txt");
        assert_eq!(&z[38..43], b"hello");
        // end record: 1 entry, central directory right after the data
        let end = &z[z.len() - 22..];
        assert_eq!(&end[..4], &0x0605_4b50u32.to_le_bytes());
        assert_eq!(u16::from_le_bytes([end[10], end[11]]), 1);
        assert_eq!(u32::from_le_bytes(end[16..20].try_into().unwrap()), 43);
        let _ = fs::remove_dir_all(&dir);
    }
}
