use std::collections::BTreeMap;

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
    pending: Option<AskRequest>,
    max_upload: usize,
}

impl ReceiverService {
    pub fn new(identity: ReceiverIdentity, max_upload: usize) -> Self {
        Self {
            identity,
            pending: None,
            max_upload,
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
                let ask = decode_ask(&request.body)?;
                events.push(ReceiverEvent::ConsentRequested(ask.clone()));
                if !consent(&ask) {
                    self.pending = None;
                    events.push(ReceiverEvent::Declined);
                    return Ok((response(401, "Unauthorized", Vec::new()), events));
                }
                self.pending = Some(ask.clone());
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
                let Some(ask) = self.pending.take() else {
                    return Ok((response(409, "Conflict", Vec::new()), events));
                };
                let content_type = request
                    .headers
                    .get("content-type")
                    .map(String::as_str)
                    .unwrap_or("application/x-cpio");
                let mut files = decode_upload(&request.body, content_type, self.max_upload)?;
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
