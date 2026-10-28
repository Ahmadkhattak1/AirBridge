use airbridge_core::{
    ArchiveEntry, ReceiverIdentity, decode_discover_response, decode_gzip_cpio, decode_upload,
    encode_discover_response, encode_gzip_cpio,
};
use flate2::{Compression, read::GzDecoder, write::ZlibEncoder};
use std::io::{Read, Write};

fn raw_archive() -> Vec<u8> {
    let gz = encode_gzip_cpio(&[ArchiveEntry {
        path: "original.HEIC".into(),
        mode: 0o100644,
        data: b"opaque original media bytes".to_vec(),
    }])
    .unwrap();
    let mut raw = Vec::new();
    GzDecoder::new(gz.as_slice()).read_to_end(&mut raw).unwrap();
    assert_eq!(&raw[..6], b"070707");
    raw
}

#[test]
fn dvzip_mixed_stored_and_zlib_chunks_keep_media_bytes() {
    let raw = raw_archive();
    let split = 87; // Split inside the next CPIO header, not at an entry boundary.
    let mut body = ((split as u32) | 0x80000000).to_be_bytes().to_vec();
    body.extend_from_slice(&raw[..split]);
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&raw[split..]).unwrap();
    let compressed = z.finish().unwrap();
    body.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    body.extend(compressed);
    let files = decode_upload(&body, "application/x-dvzip", 4096).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "original.HEIC");
    assert_eq!(files[0].data, b"opaque original media bytes");
    assert!(decode_upload(&body[..body.len() - 1], "application/x-dvzip", 4096).is_err());
    assert!(decode_upload(&body, "application/x-dvzip", 32).is_err());
    assert!(decode_upload(&[0, 0, 0, 0], "application/x-dvzip", 4096).is_err());
}

#[test]
fn gzip_checksum_and_truncation_are_not_accepted() {
    let mut body = encode_gzip_cpio(&[ArchiveEntry {
        path: "test.txt".into(),
        mode: 0o100644,
        data: vec![7; 500],
    }])
    .unwrap();
    assert!(decode_gzip_cpio(&body[..body.len() - 4], 4096).is_err());
    let crc = body.len() - 8;
    body[crc] ^= 1;
    assert!(decode_gzip_cpio(&body, 4096).is_err());
}

#[test]
fn receiver_declares_original_heif_and_hevc_support() {
    let bytes = encode_discover_response(&ReceiverIdentity {
        computer_name: "AirBridge".into(),
        model_name: "Windows PC".into(),
    })
    .unwrap();
    let response = decode_discover_response(&bytes).unwrap();
    let caps: serde_json::Value = serde_json::from_slice(&response.media_capabilities).unwrap();
    assert!(
        caps["ContainerFormats"]["public.heif-standard"]["HeifSubtypes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "public.heic")
    );
    assert!(
        caps["Codecs"]["hvc1"]["Profiles"]["VTSupportedProfiles"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(2))
    );
}

#[test]
fn windows_reserved_paths_are_refused_also_on_linux() {
    for name in [
        "../escape",
        "C:escape",
        "x:stream",
        "CON.txt",
        "dir\\escape",
        "x.",
    ] {
        assert!(
            encode_gzip_cpio(&[ArchiveEntry {
                path: name.into(),
                mode: 0o100644,
                data: vec![]
            }])
            .is_err(),
            "{name}"
        );
    }
}
