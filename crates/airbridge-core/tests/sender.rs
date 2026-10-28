use std::io::Cursor;

use airbridge_core::{AskRequest, FileEntry, encode_ask_for_transfer, read_response};
use plist::Value;

#[test]
fn current_ask_links_the_upload_and_describes_original_file_size() {
    let ask = AskRequest {
        sender_name: "Windows PC".into(),
        sender_model: "Windows PC".into(),
        sender_id: "00e0253fff2f".into(),
        files: vec![FileEntry {
            name: "photo.heic".into(),
            uti: "public.heic".into(),
            bom_path: "./photo.heic".into(),
            is_directory: false,
        }],
        items: vec![],
        convert_media: false,
        icon: None,
    };
    let id = "A3F34E01-1234-4678-ABCD-EF0123456789";
    let encoded = encode_ask_for_transfer(&ask, id, &[1_376_048]).unwrap();
    let value = Value::from_reader(Cursor::new(encoded)).unwrap();
    let d = value.as_dictionary().unwrap();
    assert_eq!(
        d["TransferID"].as_dictionary().unwrap()["id"].as_string(),
        Some(id)
    );
    assert!(
        d["TransferType"].as_dictionary().unwrap()["files"]
            .as_dictionary()
            .is_some()
    );
    let f = d["Files"].as_array().unwrap()[0].as_dictionary().unwrap();
    assert_eq!(f["FileSize"].as_unsigned_integer(), Some(1_376_048));
    assert_eq!(f["ShouldConvertMediaFormats"].as_boolean(), Some(false));
    assert_eq!(f["FileBomPath"].as_string(), Some("./photo.heic"));
    assert!(!f.contains_key("ConvertMediaFormats"));
    assert!(encode_ask_for_transfer(&ask, id, &[]).is_err());
}

#[test]
fn closing_without_upload_acknowledgement_is_not_reported_as_version_failure() {
    let error = read_response(Cursor::new(Vec::<u8>::new()), 1024).unwrap_err();
    assert!(error.to_string().contains("peer closed"));
    let response = read_response(
        Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        1024,
    )
    .unwrap();
    assert_eq!(response.status, 200);
    assert!(response.body.is_empty());
}
