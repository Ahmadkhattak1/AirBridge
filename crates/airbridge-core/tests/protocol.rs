use std::collections::BTreeMap;
use std::io::Cursor;

use airbridge_core::*;

#[test]
fn ask_round_trip_preserves_sender_and_files() {
    let ask = AskRequest {
        sender_name: "My iPhone".into(),
        sender_model: "iPhone".into(),
        sender_id: "abc123".into(),
        files: vec![FileEntry {
            name: "photo.heic".into(),
            uti: "public.heic".into(),
            bom_path: "./photo.heic".into(),
            is_directory: false,
        }],
        items: vec![],
        convert_media: false,
        icon: Some(vec![1, 2, 3]),
    };
    assert_eq!(decode_ask(&encode_ask(&ask).unwrap()).unwrap(), ask);
}

#[test]
fn gzip_cpio_round_trip_and_rejects_traversal() {
    let entries = vec![ArchiveEntry {
        path: "photos/a.jpg".into(),
        data: b"original".to_vec(),
        mode: 0o100644,
    }];
    let archive = encode_gzip_cpio(&entries).unwrap();
    assert_eq!(decode_gzip_cpio(&archive, 1024).unwrap(), entries);
    assert!(
        encode_gzip_cpio(&[ArchiveEntry {
            path: "../escape".into(),
            data: vec![],
            mode: 0o100644
        }])
        .is_err()
    );
}

#[test]
fn http_round_trip_is_binary_safe() {
    let request = HttpRequest {
        method: "POST".into(),
        path: "/Ask".into(),
        headers: BTreeMap::from([("content-type".into(), "application/octet-stream".into())]),
        body: vec![0, 255, 10, 13],
    };
    let mut wire = Vec::new();
    write_request(&mut wire, &request).unwrap();
    let decoded = read_request(Cursor::new(wire), 1024).unwrap();
    assert_eq!(decoded.method, request.method);
    assert_eq!(decoded.path, request.path);
    assert_eq!(decoded.body, request.body);
    assert_eq!(
        decoded.headers.get("content-type"),
        request.headers.get("content-type")
    );
}

#[test]
fn discover_response_round_trip() {
    let id = ReceiverIdentity {
        computer_name: "AirBridge".into(),
        model_name: "WindowsPC".into(),
    };
    assert_eq!(
        decode_discover_response(&encode_discover_response(&id).unwrap())
            .unwrap()
            .identity,
        id
    );
}

#[test]
fn http_body_limit_is_enforced() {
    let wire = b"POST /Upload HTTP/1.1\r\nContent-Length: 20\r\n\r\n01234567890123456789";
    assert!(read_request(Cursor::new(wire), 10).is_err());
}

#[test]
fn receiver_requires_consent_before_upload() {
    let id = ReceiverIdentity {
        computer_name: "AirBridge".into(),
        model_name: "WindowsPC".into(),
    };
    let mut service = ReceiverService::new(id, 1024 * 1024);
    let archive = encode_gzip_cpio(&[ArchiveEntry {
        path: "hello.txt".into(),
        data: b"hello".to_vec(),
        mode: 0o100644,
    }])
    .unwrap();
    let upload = HttpRequest {
        method: "POST".into(),
        path: "/Upload".into(),
        headers: BTreeMap::new(),
        body: archive.clone(),
    };
    assert_eq!(
        service.handle(upload.clone(), |_| true).unwrap().0.status,
        409
    );

    let ask = AskRequest {
        sender_name: "Phone".into(),
        sender_model: "iPhone".into(),
        sender_id: "1".into(),
        files: vec![FileEntry {
            name: "hello.txt".into(),
            uti: "public.plain-text".into(),
            bom_path: "./hello.txt".into(),
            is_directory: false,
        }],
        items: vec![],
        convert_media: false,
        icon: None,
    };
    let ask_http = HttpRequest {
        method: "POST".into(),
        path: "/Ask".into(),
        headers: BTreeMap::new(),
        body: encode_ask(&ask).unwrap(),
    };
    assert_eq!(service.handle(ask_http, |_| true).unwrap().0.status, 200);
    let (_, events) = service.handle(upload, |_| true).unwrap();
    assert!(matches!(&events[0], ReceiverEvent::FilesReceived(files) if files[0].data == b"hello"));
}
