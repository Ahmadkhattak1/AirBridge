use std::io::Write;

use flate2::{Compression, write::GzEncoder};
use owly_airdrop::proto::archive::{
    compress::{Compressor, Decoder},
    cpio,
};

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub path: String,
    pub data: Vec<u8>,
    pub mode: u32,
}

/// Apple uses odc, including a root directory, rather than newc on send.
pub fn encode_gzip_cpio(entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    gz.write_all(&cpio::odc_header(&cpio_entry(".", 0o040755, 0, 1)))?;
    for (index, entry) in entries.iter().enumerate() {
        validate_path(&entry.path)?;
        if entry.data.len() as u64 > cpio::ODC_MAX_SIZE {
            return Err(Error::Archive("file too large for CPIO".into()));
        }
        gz.write_all(&cpio::odc_header(&cpio_entry(
            &format!("./{}", entry.path),
            entry.mode,
            entry.data.len() as u64,
            index as u32 + 2,
        )))?;
        gz.write_all(&entry.data)?;
    }
    gz.write_all(&cpio::odc_trailer())?;
    Ok(gz.finish()?)
}

fn cpio_entry(name: &str, mode: u32, size: u64, ino: u32) -> cpio::Entry {
    cpio::Entry {
        name: name.into(),
        mode,
        size,
        ino,
        nlink: 1,
        uid: 0,
        gid: 0,
        mtime: 0,
    }
}

pub fn decode_gzip_cpio(bytes: &[u8], limit: usize) -> Result<Vec<ArchiveEntry>> {
    decode_upload(bytes, "application/x-cpio", limit)
}

/// Compression is selected from the HTTP type, never guessed from a failed decode.
pub fn decode_upload(bytes: &[u8], content_type: &str, limit: usize) -> Result<Vec<ArchiveEntry>> {
    let kind = Compressor::from_content_type(content_type)
        .ok_or_else(|| Error::Archive(format!("unsupported upload type: {content_type}")))?;
    let mut decoder = Decoder::new(kind, 1024 * 1024);
    let mut raw = Vec::new();
    let mut at = 0;
    let mut block = [0u8; 65536];
    loop {
        let progress = decoder
            .decode(&bytes[at..], &mut block)
            .map_err(|e| Error::Archive(e.to_string()))?;
        at += progress.consumed;
        if raw.len().saturating_add(progress.produced) > limit {
            return Err(Error::Archive("archive exceeds size limit".into()));
        }
        raw.extend_from_slice(&block[..progress.produced]);
        if progress.consumed == 0 && progress.produced == 0 {
            break;
        }
    }
    if !decoder.at_boundary() {
        return Err(Error::Archive("truncated compressed archive".into()));
    }
    let mut reader = cpio::Reader::new();
    let mut at = 0;
    let mut files = Vec::new();
    let mut current: Option<ArchiveEntry> = None;
    let mut names = std::collections::HashSet::new();
    loop {
        let (used, event) = reader
            .feed(&raw[at..])
            .map_err(|e| Error::Archive(e.to_string()))?;
        at += used;
        match event {
            Some(cpio::Event::Entry(entry)) => {
                if entry.name == "." || entry.name == "./" {
                    if !entry.is_dir() || entry.size != 0 {
                        return Err(Error::Archive("invalid archive root".into()));
                    }
                    continue;
                }
                let name = entry.name.strip_prefix("./").unwrap_or(&entry.name);
                validate_path(name)?;
                if (!entry.is_file() && !entry.is_dir()) || (entry.is_file() && entry.nlink > 1) {
                    return Err(Error::Archive(
                        "links and special files are unsupported".into(),
                    ));
                }
                if entry.size > limit as u64 || (entry.is_dir() && entry.size != 0) {
                    return Err(Error::Archive("invalid file size".into()));
                }
                if !names.insert(name.to_owned()) || names.len() > 4096 {
                    return Err(Error::Archive("duplicate entry or too many files".into()));
                }
                current = Some(ArchiveEntry {
                    path: name.into(),
                    data: Vec::new(),
                    mode: entry.mode,
                });
            }
            Some(cpio::Event::Data(data)) => {
                current
                    .as_mut()
                    .ok_or_else(|| Error::Archive("data without a file".into()))?
                    .data
                    .extend_from_slice(data);
            }
            Some(cpio::Event::EntryEnd) => {
                if let Some(file) = current.take() {
                    files.push(file);
                }
            }
            Some(cpio::Event::Trailer) => return Ok(files),
            None if used == 0 => return Err(Error::Archive("truncated CPIO archive".into())),
            None => {}
        }
    }
}

pub(crate) fn validate_path(value: &str) -> Result<()> {
    // The Linux receiver writes to Windows; enforce Windows rules on both OSes.
    if value.is_empty()
        || value.len() > 1024
        || value.split('/').any(|part| {
            let stem = part
                .split('.')
                .next()
                .unwrap_or("")
                .trim_end()
                .to_ascii_uppercase();
            let port = stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"));
            part.is_empty()
                || matches!(part, "." | "..")
                || part.len() > 255
                || part.ends_with(['.', ' '])
                || part
                    .chars()
                    .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || port.is_some_and(|p| p.len() == 1 && p.as_bytes()[0].is_ascii_digit())
        })
    {
        return Err(Error::UnsafePath(value.into()));
    }
    Ok(())
}
