use airbridge_core::*;

fn service() -> ReceiverService {
    ReceiverService::new(
        ReceiverIdentity {
            computer_name: "PC".into(),
            model_name: "Windows".into(),
        },
        1024 * 1024,
    )
}

fn ask() -> AskRequest {
    AskRequest {
        sender_name: "Phone".into(),
        sender_model: "iPhone".into(),
        sender_id: "peer-1".into(),
        files: vec![FileEntry {
            name: "video.mov".into(),
            bom_path: "./video.mov".into(),
            uti: "com.apple.quicktime-movie".into(),
            is_directory: false,
        }],
        items: vec![],
        convert_media: false,
        icon: None,
    }
}

fn offer(id: &str, size: u64) -> HttpRequest {
    HttpRequest {
        method: "POST".into(),
        path: "/Ask".into(),
        headers: Default::default(),
        body: encode_ask_for_transfer(&ask(), id, &[size]).unwrap(),
    }
}

fn upload(id: &str) -> HttpRequest {
    HttpRequest {
        method: "POST".into(),
        path: "/Upload".into(),
        headers: [("transferid".into(), id.into())].into(),
        body: vec![],
    }
}

#[test]
fn repeated_offers_before_and_during_upload_prompt_once() {
    let mut service = service();
    let request = offer("transfer-1", 900_000_000);
    let (_, events) = service.handle(request.clone(), |_| true).unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ReceiverEvent::Accepted(_)))
    );
    let (response, events) = service
        .handle(request.clone(), |_| panic!("duplicate prompt"))
        .unwrap();
    assert_eq!(response.status, 200);
    assert!(events.is_empty());
    assert!(service.begin_upload(&upload("transfer-1")).is_some());
    assert!(
        service.begin_upload(&upload("transfer-1")).is_none(),
        "no concurrent duplicate upload"
    );
    for _ in 0..8 {
        let (response, events) = service
            .handle(request.clone(), |_| panic!("mid-video duplicate prompt"))
            .unwrap();
        assert_eq!(response.status, 200);
        assert!(events.is_empty());
    }
    service.finish_upload(true);
    assert!(service.begin_upload(&upload("transfer-1")).is_none());
    service.handle(request, |_| false).unwrap(); // Fresh consent after completion.
    assert!(service.begin_upload(&upload("transfer-1")).is_none());
}

#[test]
fn reconnect_reuses_consent_but_different_transfer_or_size_cannot_replace_it() {
    let mut service = service();
    service.handle(offer("one", 100), |_| true).unwrap();
    assert!(service.begin_upload(&upload("wrong")).is_none());
    assert!(service.begin_upload(&upload("one")).is_some());
    service.finish_upload(false); // Failed TLS body; a new connection may retry.
    assert!(
        service
            .handle(offer("one", 100), |_| panic!("retry prompted"))
            .unwrap()
            .1
            .is_empty()
    );
    assert!(service.begin_upload(&upload("one")).is_some());
    for request in [offer("two", 100), offer("one", 200)] {
        assert_eq!(
            service
                .handle(request, |_| panic!("offer replaced in-flight transfer"))
                .unwrap()
                .0
                .status,
            409
        );
    }
    assert!(service.begin_upload(&upload("one")).is_none());
}

#[test]
fn another_peer_or_unsafe_offer_never_reuses_consent() {
    let mut first = service();
    first.handle(offer("one", 100), |_| true).unwrap();
    assert!(service().begin_upload(&upload("one")).is_none());
    let mut unsafe_ask = ask();
    unsafe_ask.files[0].bom_path = "../outside.mov".into();
    let request = HttpRequest {
        body: encode_ask(&unsafe_ask).unwrap(),
        ..offer("two", 100)
    };
    assert!(
        service()
            .handle(request, |_| panic!("unsafe offer prompted"))
            .is_err()
    );
}
