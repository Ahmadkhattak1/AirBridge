use super::*;

fn app() -> AirBridgeApp {
    AirBridgeApp::initial(PathBuf::from("C:/Users/Public/Downloads/AirBridge"), true)
}
fn event(app: &mut AirBridgeApp, value: serde_json::Value) {
    let id = app.generation.load(Ordering::SeqCst);
    app.events_tx
        .send(Event::Line(id, value.to_string()))
        .unwrap();
    app.handle_events();
}

#[test]
fn incoming_consent_is_not_dismissed_by_control_request_bytes() {
    let mut app = app();
    app.listening = true;
    event(
        &mut app,
        serde_json::json!({"event":"approval-needed","sender":"Test iPhone","files":2}),
    );
    event(
        &mut app,
        serde_json::json!({"event":"receiving","bytes":450,"total":450}),
    );
    assert_eq!(app.phase, Phase::Consent);
    assert_eq!(app.pending_sender.as_deref(), Some("Test iPhone"));
    assert_eq!(app.bytes, 0);
    event(&mut app, serde_json::json!({"event":"accepted"}));
    event(
        &mut app,
        serde_json::json!({"event":"receiving","bytes":12000,"total":25000}),
    );
    assert_eq!(app.phase, Phase::Transfer);
    assert_eq!((app.bytes, app.total), (12000, Some(25000)));
    event(
        &mut app,
        serde_json::json!({"event":"received","files":["one.heic","two.mov"]}),
    );
    assert_eq!(app.phase, Phase::Complete);
    assert!(app.listening);
    assert_eq!(app.incoming_count, 2);
}

#[test]
fn upload_size_is_not_presented_as_outgoing_progress_or_success() {
    let mut app = app();
    app.view = View::Send;
    app.sending = true;
    event(
        &mut app,
        serde_json::json!({"event":"send-target","id":"1","name":"Test iPhone"}),
    );
    assert_eq!(app.send_targets.len(), 1);
    event(&mut app, serde_json::json!({"event":"asking"}));
    event(
        &mut app,
        serde_json::json!({"event":"send-target","id":"2","name":"Late device"}),
    );
    assert_eq!(app.phase, Phase::Waiting);
    assert_eq!(app.send_targets.len(), 1);
    event(
        &mut app,
        serde_json::json!({"event":"sending","bytes":50000}),
    );
    assert_eq!(app.phase, Phase::Transfer);
    assert_eq!(app.bytes, 0);
    assert!(app.sending);
    event(&mut app, serde_json::json!({"event":"sent","files":1}));
    assert_eq!(app.phase, Phase::Complete);
    assert!(!app.sending);
}

#[test]
fn cancelled_session_cannot_replace_the_home_screen() {
    let mut app = app();
    let old_id = app.generation.load(Ordering::SeqCst);
    app.stop();
    app.events_tx
        .send(Event::Line(
            old_id,
            serde_json::json!({"event":"approval-needed","sender":"Old session","files":1})
                .to_string(),
        ))
        .unwrap();
    app.events_tx
        .send(Event::Stopped(old_id, "Old error".into()))
        .unwrap();
    app.handle_events();
    assert_eq!(app.view, View::Home);
    assert_eq!(app.phase, Phase::Idle);
    assert!(app.pending_sender.is_none());
}

#[test]
fn selected_local_folder_retains_spaces_and_unicode_for_runtime() {
    assert_eq!(
        windows_to_wsl("D:\\My photos\\سفر"),
        Some("/mnt/d/My photos/سفر".into())
    );
    assert!(windows_to_wsl("\\\\server\\share").is_none());
}
