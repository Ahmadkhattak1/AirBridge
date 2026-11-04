use std::collections::HashSet;

use owly_airdrop::proto::archive::{
    compress::{Compressor, Decoder},
    cpio,
};

use crate::{Error, Result, archive::validate_path};

#[derive(Clone, Debug)]
pub struct UploadEntry {
    pub path: String,
    pub mode: u32,
    pub size: u64,
}

/// Files remain private to the sink until finish validates the whole archive.
pub trait UploadSink {
    fn start(&mut self, entry: UploadEntry) -> Result<()>;
    fn data(&mut self, bytes: &[u8]) -> Result<()>;
    fn end(&mut self) -> Result<()>;
}

/// Incremental gzip/DVZip and CPIO decoding. Memory is independent of file size.
pub struct UploadDecoder {
    decoder: Decoder,
    cpio: cpio::Reader,
    names: HashSet<String>,
    decoded: usize,
    limit: usize,
    skip: bool,
}

impl UploadDecoder {
    pub fn new(content_type: &str, limit: usize) -> Result<Self> {
        let kind = Compressor::from_content_type(content_type)
            .ok_or_else(|| Error::Archive(format!("unsupported upload type: {content_type}")))?;
        Ok(Self {
            decoder: Decoder::new(kind, 1024 * 1024),
            cpio: cpio::Reader::new(),
            names: HashSet::new(),
            decoded: 0,
            limit,
            skip: true,
        })
    }

    pub fn feed(&mut self, input: &[u8], sink: &mut impl UploadSink) -> Result<()> {
        let mut at = 0;
        let mut block = [0; 64 * 1024];
        loop {
            let progress = self
                .decoder
                .decode(&input[at..], &mut block)
                .map_err(|e| Error::Archive(e.to_string()))?;
            at += progress.consumed;
            self.decoded = self
                .decoded
                .checked_add(progress.produced)
                .filter(|n| *n <= self.limit)
                .ok_or_else(|| Error::Archive("archive exceeds size limit".into()))?;
            let mut pos = 0;
            loop {
                let (used, event) = self
                    .cpio
                    .feed(&block[pos..progress.produced])
                    .map_err(|e| Error::Archive(e.to_string()))?;
                pos += used;
                match event {
                    Some(cpio::Event::Entry(entry)) => {
                        self.skip = true;
                        if entry.name == "." || entry.name == "./" {
                            if !entry.is_dir() || entry.size != 0 {
                                return Err(Error::Archive("invalid archive root".into()));
                            }
                            continue;
                        }
                        let name = entry.name.strip_prefix("./").unwrap_or(&entry.name);
                        validate_path(name)?;
                        if (!entry.is_file() && !entry.is_dir())
                            || (entry.is_file() && entry.nlink > 1)
                        {
                            return Err(Error::Archive(
                                "links and special files are unsupported".into(),
                            ));
                        }
                        if entry.size > self.limit as u64 || (entry.is_dir() && entry.size != 0) {
                            return Err(Error::Archive("invalid file size".into()));
                        }
                        if !self.names.insert(name.to_lowercase()) || self.names.len() > 4096 {
                            return Err(Error::Archive("duplicate entry or too many files".into()));
                        }
                        self.skip = name.rsplit('/').next().unwrap_or("").starts_with("._");
                        if !self.skip {
                            sink.start(UploadEntry {
                                path: name.into(),
                                mode: entry.mode,
                                size: entry.size,
                            })?;
                        }
                    }
                    Some(cpio::Event::Data(data)) if !self.skip => sink.data(data)?,
                    Some(cpio::Event::EntryEnd) if !self.skip => sink.end()?,
                    None if used == 0 => break,
                    _ => {}
                }
            }
            if progress.consumed == 0 && progress.produced == 0 {
                if at != input.len() {
                    return Err(Error::Archive("compressed archive stalled".into()));
                }
                return Ok(());
            }
        }
    }

    pub fn is_complete(&self) -> bool {
        self.decoder.at_boundary() && self.cpio.is_done()
    }

    pub fn finish(mut self, sink: &mut impl UploadSink) -> Result<()> {
        self.feed(&[], sink)?;
        if !self.is_complete() {
            return Err(Error::Archive("truncated archive".into()));
        }
        Ok(())
    }
}
