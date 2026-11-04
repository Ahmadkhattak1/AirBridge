//! AirDrop-compatible application protocol implemented independently from the
//! public protocol description and interoperability traces.

mod archive;
mod http;
mod plist_protocol;
mod service;
mod streaming;

pub use archive::{ArchiveEntry, decode_gzip_cpio, decode_upload, encode_gzip_cpio};
pub use http::{
    HttpRequest, HttpResponse, read_request, read_request_body_to, read_request_body_until,
    read_request_buffered, read_request_buffered_continue, read_request_buffered_progress,
    read_request_head, read_response, read_response_buffered, write_request, write_response,
};
pub use plist_protocol::{
    AskRequest, DiscoverResponse, FileEntry, ReceiverIdentity, decode_ask,
    decode_discover_response, encode_ask, encode_ask_for_transfer, encode_discover_request,
    encode_discover_response, encode_receiver_identity, extension_uti,
};
pub use service::{ReceiverEvent, ReceiverService};
pub use streaming::{UploadDecoder, UploadEntry, UploadSink};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid HTTP: {0}")]
    Http(String),
    #[error("invalid property list: {0}")]
    Plist(#[from] plist::Error),
    #[error("invalid archive: {0}")]
    Archive(String),
    #[error("unsafe file name: {0}")]
    UnsafePath(String),
}

pub type Result<T> = std::result::Result<T, Error>;
