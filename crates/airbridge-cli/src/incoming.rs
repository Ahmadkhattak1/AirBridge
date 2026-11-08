use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use airbridge_core::{
    AskRequest, HttpRequest, HttpResponse, ReceiverService, UploadDecoder, UploadEntry, UploadSink,
    read_request_body_until,
};
use anyhow::{Context, Result, ensure};

pub const MAX_UPLOAD: usize = 8 * 1024 * 1024 * 1024;

struct Reservation {
    service: Arc<Mutex<ReceiverService>>,
    active: bool,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.active {
            self.service
                .lock()
                .expect("receiver state poisoned")
                .finish_upload(false);
        }
    }
}

pub fn receive_upload<S: Read + Write>(
    reader: &mut BufReader<S>,
    request: &HttpRequest,
    service: Arc<Mutex<ReceiverService>>,
    directory: &Path,
) -> Result<HttpResponse> {
    receive_upload_from(request, service, directory, |sink| {
        read_request_body_until(reader, request, MAX_UPLOAD, |_, _| {}, sink)
    })
}

pub fn receive_upload_from(
    request: &HttpRequest,
    service: Arc<Mutex<ReceiverService>>,
    directory: &Path,
    read_body: impl FnOnce(
        &mut dyn FnMut(&[u8]) -> airbridge_core::Result<ControlFlow<()>>,
    ) -> airbridge_core::Result<()>,
) -> Result<HttpResponse> {
    let ask = service
        .lock()
        .expect("receiver state poisoned")
        .begin_upload(request);
    let Some(ask) = ask else {
        return Ok(HttpResponse {
            status: 409,
            reason: "Conflict".into(),
            headers: [("connection".into(), "close".into())].into(),
            body: Vec::new(),
        });
    };
    let mut reservation = Reservation {
        service,
        active: true,
    };
    let mut staged = StagedUpload::new(directory, ask)?;
    let content_type = request
        .headers
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("application/x-cpio");
    let mut decoder = UploadDecoder::new(content_type, MAX_UPLOAD)?;
    let started = Instant::now();
    let mut progress_at = started - Duration::from_secs(1);
    let mut received = 0;
    println!(
        "{}",
        serde_json::json!({"event":"upload-started", "transfer_id":request.headers.get("transferid"),
            "content_type":content_type, "transfer_encoding":request.headers.get("transfer-encoding"),
            "content_length":request.headers.get("content-length")})
    );
    let total = request
        .headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok());
    let body_result = read_body(&mut |chunk| {
        received += chunk.len();
        if received > MAX_UPLOAD {
            return Err(airbridge_core::Error::Http(
                "upload exceeds size limit".into(),
            ));
        }
        if progress_at.elapsed() >= Duration::from_millis(500) {
            println!(
                "{}",
                serde_json::json!({"event":"receiving", "bytes":received, "total":total,
                "elapsed_ms":started.elapsed().as_millis(), "bytes_per_second":received as f64 / started.elapsed().as_secs_f64().max(0.001)})
            );
            progress_at = Instant::now();
        }
        decoder.feed(chunk, &mut staged)?;
        Ok(if decoder.is_complete() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        })
    });
    if let Err(error) = body_result {
        let archive_complete = decoder.finish(&mut staged).is_ok();
        return Err(error).with_context(|| format!(
            "The file transfer stopped after {received} bytes in {} seconds (archive complete: {archive_complete})",
            started.elapsed().as_secs()
        ));
    }
    decoder.finish(&mut staged)?;
    let saved = staged.commit(directory)?;
    reservation.active = false;
    reservation
        .service
        .lock()
        .expect("receiver state poisoned")
        .finish_upload(true);
    println!(
        "{}",
        serde_json::json!({"event":"received", "files":saved, "bytes":received,
        "elapsed_ms":started.elapsed().as_millis()})
    );
    Ok(HttpResponse {
        status: 200,
        reason: "OK".into(),
        headers: [("connection".into(), "close".into())].into(),
        body: Vec::new(),
    })
}

/// Write privately beside the destination. Only a fully validated archive is
/// published; disconnects, bad checksums and rejected entries remove the parts.
struct StagedUpload {
    root: PathBuf,
    ask: AskRequest,
    current: Option<BufWriter<File>>,
    files: Vec<String>,
}

impl StagedUpload {
    fn new(directory: &Path, ask: AskRequest) -> Result<Self> {
        let root = directory.join(format!(".airbridge-partial-{}", super::new_transfer_id()?));
        fs::create_dir(&root)?;
        Ok(Self {
            root,
            ask,
            current: None,
            files: Vec::new(),
        })
    }

    fn commit(&mut self, directory: &Path) -> Result<Vec<String>> {
        // TCP and QUIC may finish different approved uploads together. Keep
        // name selection and publication atomic relative to other uploads.
        static PUBLISH: Mutex<()> = Mutex::new(());
        let _publish = PUBLISH.lock().expect("upload publication lock poisoned");
        for offered in &self.ask.files {
            let path = offered
                .bom_path
                .strip_prefix("./")
                .unwrap_or(&offered.bom_path);
            ensure!(
                self.root.join(path).exists(),
                "The upload did not include the offered file: {}",
                offered.name
            );
        }
        let mut saved = Vec::new();
        for offered in &self.ask.files {
            let path = offered
                .bom_path
                .strip_prefix("./")
                .unwrap_or(&offered.bom_path);
            let target = super::unique_path(directory, path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let source = self.root.join(path);
            // Overlapping folder offers may already have moved this entry.
            if !source.exists() {
                continue;
            }
            fs::rename(source, &target)?;
            if offered.is_directory {
                for file in &self.files {
                    if let Some(relative) = file.strip_prefix(&format!("{path}/")) {
                        saved.push(target.join(relative).display().to_string());
                    }
                }
            } else {
                saved.push(target.display().to_string());
            }
        }
        Ok(saved)
    }
}

impl UploadSink for StagedUpload {
    fn start(&mut self, entry: UploadEntry) -> airbridge_core::Result<()> {
        let allowed = self.ask.files.iter().any(|allowed| {
            let path = allowed
                .bom_path
                .strip_prefix("./")
                .unwrap_or(&allowed.bom_path);
            entry.path == path
                || (allowed.is_directory && entry.path.starts_with(&format!("{path}/")))
        });
        if !allowed {
            return Err(airbridge_core::Error::Archive(format!(
                "file was not offered: {}",
                entry.path
            )));
        }
        let target = self.root.join(&entry.path);
        if entry.mode & 0o170000 == 0o040000 {
            fs::create_dir_all(target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            self.current = Some(BufWriter::with_capacity(
                1024 * 1024,
                File::create_new(target)?,
            ));
            self.files.push(entry.path);
        }
        Ok(())
    }

    fn data(&mut self, bytes: &[u8]) -> airbridge_core::Result<()> {
        self.current
            .as_mut()
            .ok_or_else(|| airbridge_core::Error::Archive("data without a file".into()))?
            .write_all(bytes)?;
        Ok(())
    }

    fn end(&mut self) -> airbridge_core::Result<()> {
        if let Some(mut file) = self.current.take() {
            file.flush()?;
        }
        Ok(())
    }
}

impl Drop for StagedUpload {
    fn drop(&mut self) {
        self.current = None;
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use airbridge_core::{
        ArchiveEntry, FileEntry, ReceiverIdentity, encode_ask_for_transfer, encode_gzip_cpio,
    };
    use std::io::Cursor;

    fn ask() -> AskRequest {
        AskRequest {
            sender_name: "Phone".into(),
            sender_model: "iPhone".into(),
            sender_id: "peer".into(),
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

    fn offered(service: &mut ReceiverService) -> HttpRequest {
        let request = HttpRequest {
            method: "POST".into(),
            path: "/Ask".into(),
            headers: Default::default(),
            body: encode_ask_for_transfer(&ask(), "same-transfer", &[200_000]).unwrap(),
        };
        service.handle(request.clone(), |_| true).unwrap();
        request
    }

    struct Wire {
        input: Cursor<Vec<u8>>,
        service: Arc<Mutex<ReceiverService>>,
        offer: HttpRequest,
        repeated: bool,
        output: Vec<u8>,
    }
    impl Read for Wire {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if self.input.position() > 0 && !self.repeated {
                self.repeated = true;
                // The upload must not hold the service lock. This models a
                // repeated /Ask on another TLS connection during the body.
                let mut service = self.service.try_lock().expect("upload held receiver lock");
                let (response, events) = service
                    .handle(self.offer.clone(), |_| panic!("second prompt"))
                    .unwrap();
                assert_eq!(response.status, 200);
                assert!(events.is_empty());
            }
            let n = bytes.len().min(128);
            self.input.read(&mut bytes[..n])
        }
    }
    impl Write for Wire {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn fixture(body: Vec<u8>, offered_length: usize) -> (BufReader<Wire>, HttpRequest) {
        let mut service = ReceiverService::new(
            ReceiverIdentity {
                computer_name: "PC".into(),
                model_name: "Windows".into(),
            },
            MAX_UPLOAD,
        );
        let offer = offered(&mut service);
        let wire = Wire {
            input: Cursor::new(body),
            service: Arc::new(Mutex::new(service)),
            offer,
            repeated: false,
            output: vec![],
        };
        let request = HttpRequest {
            method: "POST".into(),
            path: "/Upload".into(),
            headers: [
                ("transferid".into(), "same-transfer".into()),
                ("content-length".into(), offered_length.to_string()),
                ("expect".into(), "100-continue".into()),
            ]
            .into(),
            body: vec![],
        };
        (BufReader::new(wire), request)
    }

    fn body(name: &str) -> Vec<u8> {
        encode_gzip_cpio(&[ArchiveEntry {
            path: name.into(),
            mode: 0o100644,
            data: vec![42; 200_000],
        }])
        .unwrap()
    }

    #[test]
    fn upload_repeated_offer_saves_once_and_preserves_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("video.mov"), b"existing").unwrap();
        let body = body("video.mov");
        let (mut reader, request) = fixture(body.clone(), body.len());
        let service = reader.get_ref().service.clone();
        assert_eq!(
            receive_upload(&mut reader, &request, service.clone(), dir.path())
                .unwrap()
                .status,
            200
        );
        assert!(reader.get_ref().repeated);
        assert_eq!(fs::read(dir.path().join("video.mov")).unwrap(), b"existing");
        assert_eq!(
            fs::read(dir.path().join("video (2).mov")).unwrap(),
            vec![42; 200_000]
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        assert!(service.lock().unwrap().begin_upload(&request).is_none());
    }

    #[test]
    fn corrupt_incomplete_and_unoffered_uploads_remove_parts_and_release_reservation() {
        let valid = body("video.mov");
        let mut corrupt = valid.clone();
        let crc = corrupt.len() - 8;
        corrupt[crc] ^= 1;
        for body in [
            corrupt,
            valid[..valid.len() - 4].to_vec(),
            body("unoffered.mov"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (mut reader, request) = fixture(body.clone(), body.len());
            let service = reader.get_ref().service.clone();
            assert!(receive_upload(&mut reader, &request, service.clone(), dir.path()).is_err());
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
            assert!(service.lock().unwrap().begin_upload(&request).is_some());
        }
        let dir = tempfile::tempdir().unwrap();
        let (mut reader, request) = fixture(valid[..100].to_vec(), valid.len());
        let service = reader.get_ref().service.clone();
        assert!(receive_upload(&mut reader, &request, service.clone(), dir.path()).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(service.lock().unwrap().begin_upload(&request).is_some());
    }

    #[test]
    fn validated_archive_completes_before_the_sender_finishes_its_last_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let archive = body("video.mov");
        let mut wire = format!("{:x}\r\n", archive.len() + 1024).into_bytes();
        wire.extend_from_slice(&archive);
        let (mut reader, mut request) = fixture(wire, 0);
        request.headers.remove("content-length");
        request
            .headers
            .insert("transfer-encoding".into(), "chunked".into());
        let service = reader.get_ref().service.clone();
        let reply = receive_upload(&mut reader, &request, service.clone(), dir.path()).unwrap();
        assert_eq!(reply.status, 200);
        assert_eq!(reply.headers.get("connection").unwrap(), "close");
        assert_eq!(
            fs::read(dir.path().join("video.mov")).unwrap(),
            vec![42; 200_000]
        );
        assert!(service.lock().unwrap().begin_upload(&request).is_none());
    }

    #[test]
    fn rejected_transfer_sends_no_continue_and_reads_no_body() {
        let dir = tempfile::tempdir().unwrap();
        let (mut reader, mut request) = fixture(body("video.mov"), 100);
        request
            .headers
            .insert("transferid".into(), "unapproved".into());
        let service = reader.get_ref().service.clone();
        assert_eq!(
            receive_upload(&mut reader, &request, service, dir.path())
                .unwrap()
                .status,
            409
        );
        assert_eq!(reader.get_ref().input.position(), 0);
        assert!(reader.get_ref().output.is_empty());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
