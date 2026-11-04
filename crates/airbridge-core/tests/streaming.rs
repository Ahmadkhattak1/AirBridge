use airbridge_core::*;
use flate2::{
    Compression,
    read::GzDecoder,
    write::{GzEncoder, ZlibEncoder},
};
use owly_airdrop::proto::archive::cpio;
use std::io::{BufReader, Cursor, Read, Write};

#[test]
fn chunk_extensions_allow_whitespace_and_missing_final_chunk_is_reported() {
    let prefix =
        b"POST /Upload HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3 \t;test=yes\r\nabc\r\n";
    let mut valid = prefix.to_vec();
    valid.extend_from_slice(b"0 \t;finished=yes\r\n\r\n");
    assert_eq!(read_request(valid.as_slice(), 1024).unwrap().body, b"abc");
    let error = read_request(prefix.as_slice(), 1024)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("before the final HTTP chunk after 3 bytes"),
        "{error}"
    );
    for suffix in ["\r\n", "wrong\r\n", "3", "+3\r\n"] {
        let mut invalid = prefix.to_vec();
        invalid.extend_from_slice(suffix.as_bytes());
        assert!(read_request(invalid.as_slice(), 1024).is_err());
    }
}

#[derive(Default)]
struct Sink {
    bytes: usize,
    largest_piece: usize,
    names: Vec<String>,
    ends: usize,
}
impl UploadSink for Sink {
    fn start(&mut self, e: UploadEntry) -> Result<()> {
        self.names.push(e.path);
        Ok(())
    }
    fn data(&mut self, bytes: &[u8]) -> Result<()> {
        self.bytes += bytes.len();
        self.largest_piece = self.largest_piece.max(bytes.len());
        Ok(())
    }
    fn end(&mut self) -> Result<()> {
        self.ends += 1;
        Ok(())
    }
}

fn archive() -> Vec<u8> {
    encode_gzip_cpio(&[ArchiveEntry {
        path: "video.mov".into(),
        mode: 0o100644,
        data: vec![42; 200_000],
    }])
    .unwrap()
}

fn stream(body: &[u8], kind: &str, piece: usize, limit: usize) -> Result<Sink> {
    let mut decoder = UploadDecoder::new(kind, limit)?;
    let mut sink = Sink::default();
    for part in body.chunks(piece) {
        decoder.feed(part, &mut sink)?;
    }
    decoder.finish(&mut sink)?;
    Ok(sink)
}

#[test]
fn gzip_and_dvzip_stream_across_every_header_and_file_boundary() {
    let gzip = archive();
    let mut raw = Vec::new();
    GzDecoder::new(gzip.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    let mut dvzip = Vec::new();
    for (i, part) in raw.chunks(65_536).enumerate() {
        if i % 2 == 0 {
            dvzip.extend_from_slice(&((part.len() as u32) | 0x80000000).to_be_bytes());
            dvzip.extend_from_slice(part);
        } else {
            let mut zlib = ZlibEncoder::new(Vec::new(), Compression::fast());
            zlib.write_all(part).unwrap();
            let compressed = zlib.finish().unwrap();
            dvzip.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
            dvzip.extend_from_slice(&compressed);
        }
    }
    for (body, kind) in [
        (&gzip, "application/x-cpio"),
        (&dvzip, "application/x-dvzip"),
    ] {
        for piece in [1, 3, 8192, 65_536] {
            let sink = stream(body, kind, piece, 300_000).unwrap();
            assert_eq!(sink.bytes, 200_000);
            assert_eq!(sink.names, ["video.mov"]);
            assert_eq!(sink.ends, 1);
            assert!(sink.largest_piece <= 65_536);
        }
        assert!(stream(&body[..body.len() - 1], kind, 8192, 300_000).is_err());
        assert!(stream(body, kind, 8192, 100_000).is_err());
    }
    let mut corrupt = gzip;
    let crc = corrupt.len() - 8;
    corrupt[crc] ^= 1;
    assert!(stream(&corrupt, "application/x-cpio", 8192, 300_000).is_err());
}

#[test]
fn video_larger_than_old_512_mib_limit_uses_bounded_pieces() {
    let size = 513 * 1024 * 1024;
    let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
    gzip.write_all(&cpio::odc_header(&cpio::Entry {
        name: "./large.mov".into(),
        mode: 0o100644,
        size: size as u64,
        nlink: 1,
        ino: 1,
        uid: 0,
        gid: 0,
        mtime: 0,
    }))
    .unwrap();
    let block = [9; 65_536];
    for _ in 0..size / block.len() {
        gzip.write_all(&block).unwrap();
    }
    gzip.write_all(&cpio::odc_trailer()).unwrap();
    let sink = stream(
        &gzip.finish().unwrap(),
        "application/x-cpio",
        65_536,
        size + 4096,
    )
    .unwrap();
    assert_eq!(sink.bytes, size);
    assert!(sink.largest_piece <= 65_536);
}

#[test]
fn huge_http_chunk_is_streamed_and_trailers_do_not_become_a_second_offer() {
    let mut wire = b"POST /Upload HTTP/1.1\r\nTransfer-Encoding: chunked\r\nExpect: 100-continue\r\n\r\n40000\r\n".to_vec();
    wire.extend(vec![7; 262_144]);
    wire.extend_from_slice(
        b"\r\n0\r\nChecksum: test\r\n\r\nPOST /Discover HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
    );
    struct Duplex {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Duplex {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(b)
        }
    }
    impl Write for Duplex {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut reader = BufReader::new(Duplex {
        input: Cursor::new(wire),
        output: vec![],
    });
    let head = read_request_head(&mut reader).unwrap();
    assert!(
        reader.get_ref().output.is_empty(),
        "authorize before 100 Continue"
    );
    let mut bytes = 0;
    read_request_body_to(
        &mut reader,
        &head,
        300_000,
        |_, _| {},
        |piece| {
            assert!(piece.len() <= 65_536);
            bytes += piece.len();
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(bytes, 262_144);
    assert_eq!(reader.get_ref().output, b"HTTP/1.1 100 Continue\r\n\r\n");
    assert_eq!(read_request_head(&mut reader).unwrap().path, "/Discover");
}
