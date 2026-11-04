use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use plist::Value;

use crate::{
    ArchiveEntry, AskRequest, HttpRequest, HttpResponse, ReceiverIdentity, Result, decode_ask,
    decode_upload, encode_discover_response, encode_receiver_identity,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiverEvent {
    Discovered,
    ConsentRequested(AskRequest),
    Declined,
    Accepted(AskRequest),
    FilesReceived(Vec<ArchiveEntry>),
    LinksReceived(Vec<String>),
}

pub struct ReceiverService {
    identity: ReceiverIdentity,
    pending: Option<Pending>,
    max_upload: usize,
}

#[derive(PartialEq, Eq)]
struct Offer {
    ask: AskRequest,
    transfer_id: Option<String>,
    sizes: Vec<Option<u64>>,
}

impl Offer {
    fn decode(body: &[u8]) -> Result<Self> {
        let mut ask = decode_ask(body)?;
        for file in &ask.files {
            crate::archive::validate_path(
                file.bom_path.strip_prefix("./").unwrap_or(&file.bom_path),
            )?;
        }
        // A regenerated preview image does not make a retry a new transfer.
        ask.icon = None;
        let value = Value::from_reader(std::io::Cursor::new(body))?;
        let d = value
            .as_dictionary()
            .ok_or_else(|| crate::Error::Http("invalid offer".into()))?;
        let transfer_id = d
            .get("TransferID")
            .and_then(|v| {
                v.as_string()
                    .or_else(|| v.as_dictionary()?.get("id")?.as_string())
            })
            .map(str::to_owned);
        let sizes = d
            .get("Files")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|v| v.as_dictionary()?.get("FileSize")?.as_unsigned_integer())
            .collect();
        Ok(Self {
            ask,
            transfer_id,
            sizes,
        })
    }
}

struct Pending {
    offer: Offer,
    activity: Instant,
    uploading: bool,
}

impl ReceiverService {
    pub fn new(identity: ReceiverIdentity, max_upload: usize) -> Self {
        Self {
            identity,
            pending: None,
            max_upload,
        }
    }

    fn expire(&mut self, now: Instant) {
        if self.pending.as_ref().is_some_and(|p| {
            !p.uploading && now.saturating_duration_since(p.activity) > Duration::from_secs(600)
        }) {
            self.pending = None;
        }
    }

    /// Reserve exactly one upload before reading its body. Consent is scoped to
    /// this peer's offer and its TransferID, including across TLS reconnects.
    pub fn begin_upload(&mut self, request: &HttpRequest) -> Option<AskRequest> {
        if request.method != "POST" || request.path != "/Upload" {
            return None;
        }
        self.expire(Instant::now());
        let pending = self.pending.as_mut()?;
        if pending.uploading || pending.offer.ask.files.is_empty() {
            return None;
        }
        if pending.offer.transfer_id.as_deref()
            != request.headers.get("transferid").map(String::as_str)
        {
            return None;
        }
        pending.uploading = true;
        pending.activity = Instant::now();
        Some(pending.offer.ask.clone())
    }

    /// A failed connection can retry the same accepted offer for ten minutes;
    /// a completed transfer always needs fresh consent for another upload.
    pub fn finish_upload(&mut self, success: bool) {
        if success {
            self.pending = None;
        } else if let Some(pending) = &mut self.pending {
            pending.uploading = false;
            pending.activity = Instant::now();
        }
    }

    pub fn handle<F>(
        &mut self,
        request: HttpRequest,
        mut consent: F,
    ) -> Result<(HttpResponse, Vec<ReceiverEvent>)>
    where
        F: FnMut(&AskRequest) -> bool,
    {
        let mut events = Vec::new();
        if request.method != "POST" {
            return Ok((response(405, "Method Not Allowed", Vec::new()), events));
        }
        match request.path.as_str() {
            "/Discover" => {
                events.push(ReceiverEvent::Discovered);
                Ok((
                    plist_response(200, encode_discover_response(&self.identity)?),
                    events,
                ))
            }
            "/Ask" => {
                self.expire(Instant::now());
                let offer = Offer::decode(&request.body)?;
                if let Some(pending) = &mut self.pending {
                    if pending.offer == offer {
                        pending.activity = Instant::now();
                        return Ok((
                            plist_response(200, encode_receiver_identity(&self.identity)?),
                            events,
                        ));
                    }
                    // Never replace a body in progress with an unrelated offer.
                    if pending.uploading {
                        return Ok((response(409, "Conflict", Vec::new()), events));
                    }
                }
                let ask = decode_ask(&request.body)?;
                events.push(ReceiverEvent::ConsentRequested(ask.clone()));
                if !consent(&ask) {
                    self.pending = None;
                    events.push(ReceiverEvent::Declined);
                    return Ok((response(401, "Unauthorized", Vec::new()), events));
                }
                if !ask.files.is_empty() {
                    self.pending = Some(Pending {
                        offer,
                        activity: Instant::now(),
                        uploading: false,
                    });
                }
                events.push(ReceiverEvent::Accepted(ask.clone()));
                if !ask.items.is_empty() {
                    events.push(ReceiverEvent::LinksReceived(ask.items.clone()));
                }
                Ok((
                    plist_response(200, encode_receiver_identity(&self.identity)?),
                    events,
                ))
            }
            "/Upload" => {
                let Some(ask) = self.begin_upload(&request) else {
                    return Ok((response(409, "Conflict", Vec::new()), events));
                };
                let content_type = request
                    .headers
                    .get("content-type")
                    .map(String::as_str)
                    .unwrap_or("application/x-cpio");
                let decoded = decode_upload(&request.body, content_type, self.max_upload);
                self.finish_upload(decoded.is_ok());
                let mut files = decoded?;
                files.retain(|file| !file.path.rsplit('/').next().unwrap_or("").starts_with("._"));
                for file in &files {
                    if !ask.files.iter().any(|allowed| {
                        let path = allowed
                            .bom_path
                            .strip_prefix("./")
                            .unwrap_or(&allowed.bom_path);
                        file.path == path
                            || (allowed.is_directory && file.path.starts_with(&format!("{path}/")))
                    }) {
                        return Err(crate::Error::Archive(format!(
                            "file was not offered: {}",
                            file.path
                        )));
                    }
                }
                events.push(ReceiverEvent::FilesReceived(files));
                let mut done = response(200, "OK", Vec::new());
                done.headers.insert("connection".into(), "close".into());
                Ok((done, events))
            }
            _ => Ok((response(404, "Not Found", Vec::new()), events)),
        }
    }
}

fn response(status: u16, reason: &str, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status,
        reason: reason.into(),
        headers: BTreeMap::from([("connection".into(), "keep-alive".into())]),
        body,
    }
}

fn plist_response(status: u16, body: Vec<u8>) -> HttpResponse {
    let mut result = response(status, if status == 200 { "OK" } else { "Error" }, body);
    result
        .headers
        .insert("content-type".into(), "application/octet-stream".into());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileEntry, encode_ask_for_transfer};

    #[test]
    fn idle_consent_expires_but_a_long_running_video_keeps_its_approval() {
        let mut service = ReceiverService::new(
            ReceiverIdentity {
                computer_name: "PC".into(),
                model_name: "Windows".into(),
            },
            1024,
        );
        let ask = AskRequest {
            sender_name: "Phone".into(),
            sender_model: "iPhone".into(),
            sender_id: "one".into(),
            files: vec![FileEntry {
                name: "video.mov".into(),
                bom_path: "./video.mov".into(),
                uti: "public.data".into(),
                is_directory: false,
            }],
            items: vec![],
            convert_media: false,
            icon: None,
        };
        let request = HttpRequest {
            method: "POST".into(),
            path: "/Ask".into(),
            headers: Default::default(),
            body: encode_ask_for_transfer(&ask, "one", &[1000]).unwrap(),
        };
        service.handle(request.clone(), |_| true).unwrap();
        let upload = HttpRequest {
            method: "POST".into(),
            path: "/Upload".into(),
            headers: [("transferid".into(), "one".into())].into(),
            body: vec![],
        };
        service.begin_upload(&upload).unwrap();
        service.expire(Instant::now() + Duration::from_secs(3600));
        assert!(service.pending.is_some());
        assert!(
            service
                .handle(request.clone(), |_| panic!("long upload prompted again"))
                .unwrap()
                .1
                .is_empty()
        );
        service.finish_upload(false);
        service.expire(Instant::now() + Duration::from_secs(601));
        assert!(service.pending.is_none());
        assert_eq!(service.handle(request, |_| false).unwrap().0.status, 401);
        assert!(service.begin_upload(&upload).is_none());
    }
}
