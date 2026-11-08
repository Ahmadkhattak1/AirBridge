use std::collections::BTreeMap;
use std::io::Cursor;

use plist::{Dictionary, Value};
use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub uti: String,
    pub bom_path: String,
    pub is_directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskRequest {
    pub sender_name: String,
    pub sender_model: String,
    pub sender_id: String,
    pub files: Vec<FileEntry>,
    pub items: Vec<String>,
    pub convert_media: bool,
    pub icon: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiverIdentity {
    pub computer_name: String,
    pub model_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverResponse {
    pub identity: ReceiverIdentity,
    pub media_capabilities: Vec<u8>,
}

fn binary(value: &Value) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    value.to_writer_binary(&mut out)?;
    Ok(out)
}

fn dictionary(bytes: &[u8]) -> Result<Dictionary> {
    Ok(Value::from_reader(Cursor::new(bytes))?
        .into_dictionary()
        .unwrap_or_default())
}

pub fn encode_discover_request() -> Result<Vec<u8>> {
    binary(&Value::Dictionary(Dictionary::new()))
}

pub fn encode_discover_response(identity: &ReceiverIdentity) -> Result<Vec<u8>> {
    let mut d = Dictionary::new();
    d.insert(
        "ReceiverComputerName".into(),
        identity.computer_name.clone().into(),
    );
    d.insert(
        "ReceiverModelName".into(),
        identity.model_name.clone().into(),
    );
    d.insert(
        "ReceiverMediaCapabilities".into(),
        // This is a file-preservation profile, not a Windows playback probe.
        // Photos' conversion preflight requires both HDR and the per-profile
        // acceleration hint to pass through HLG HEVC. Its simpler decoding-
        // support query accepts HDR alone but still results in transcoding.
        // See docs/MEDIA-NEGOTIATION.md for the measured positive/negative cases.
        Value::Data(include_bytes!("media-capabilities.json").to_vec()),
    );
    binary(&Value::Dictionary(d))
}

pub fn decode_discover_response(bytes: &[u8]) -> Result<DiscoverResponse> {
    let d = dictionary(bytes)?;
    Ok(DiscoverResponse {
        identity: ReceiverIdentity {
            computer_name: d
                .get("ReceiverComputerName")
                .and_then(Value::as_string)
                .unwrap_or("iPhone")
                .to_owned(),
            model_name: d
                .get("ReceiverModelName")
                .and_then(Value::as_string)
                .unwrap_or("iPhone")
                .to_owned(),
        },
        media_capabilities: d
            .get("ReceiverMediaCapabilities")
            .and_then(Value::as_data)
            .unwrap_or_default()
            .to_vec(),
    })
}

pub fn encode_receiver_identity(identity: &ReceiverIdentity) -> Result<Vec<u8>> {
    let mut d = Dictionary::new();
    d.insert(
        "ReceiverComputerName".into(),
        identity.computer_name.clone().into(),
    );
    d.insert(
        "ReceiverModelName".into(),
        identity.model_name.clone().into(),
    );
    binary(&Value::Dictionary(d))
}

pub fn encode_ask(request: &AskRequest) -> Result<Vec<u8>> {
    let mut d = Dictionary::new();
    d.insert(
        "SenderComputerName".into(),
        request.sender_name.clone().into(),
    );
    d.insert(
        "SenderModelName".into(),
        request.sender_model.clone().into(),
    );
    d.insert("SenderID".into(), request.sender_id.clone().into());
    d.insert("BundleID".into(), "com.apple.finder".into());
    d.insert("ConvertMediaFormats".into(), request.convert_media.into());
    if !request.files.is_empty() {
        d.insert(
            "Files".into(),
            Value::Array(
                request
                    .files
                    .iter()
                    .map(|f| {
                        let mut item = Dictionary::new();
                        item.insert("FileName".into(), f.name.clone().into());
                        item.insert("FileType".into(), f.uti.clone().into());
                        item.insert("FileBomPath".into(), f.bom_path.clone().into());
                        item.insert("FileIsDirectory".into(), f.is_directory.into());
                        item.insert("ConvertMediaFormats".into(), 0_i64.into());
                        Value::Dictionary(item)
                    })
                    .collect(),
            ),
        );
    }
    if !request.items.is_empty() {
        d.insert(
            "Items".into(),
            Value::Array(request.items.iter().cloned().map(Value::String).collect()),
        );
    }
    if let Some(icon) = &request.icon {
        d.insert("FileIcon".into(), Value::Data(icon.clone()));
    }
    binary(&Value::Dictionary(d))
}

/// Current AirDrop send form. Keep legacy decoding/receiving compatible.
pub fn encode_ask_for_transfer(
    request: &AskRequest,
    transfer_id: &str,
    sizes: &[u64],
) -> Result<Vec<u8>> {
    if request.files.is_empty() || sizes.len() != request.files.len() {
        return Err(crate::Error::Http(
            "one size is required for each offered file".into(),
        ));
    }
    let mut d = dictionary(&encode_ask(request)?)?;
    d.insert("BundleID".into(), "org.airbridge.windows".into());
    d.insert(
        "TransferID".into(),
        Value::Dictionary(Dictionary::from_iter([(
            "id",
            Value::String(transfer_id.to_owned()),
        )])),
    );
    d.insert(
        "TransferType".into(),
        Value::Dictionary(Dictionary::from_iter([(
            "files",
            Value::Dictionary(Dictionary::new()),
        )])),
    );
    d.insert("Items".into(), Value::Array(Vec::new()));
    if let Some(Value::Array(files)) = d.get_mut("Files") {
        for (file, size) in files.iter_mut().zip(sizes) {
            if let Value::Dictionary(file) = file {
                file.insert("FileSize".into(), Value::Integer((*size).into()));
                file.insert("ShouldConvertMediaFormats".into(), false.into());
                file.remove("ConvertMediaFormats");
            }
        }
    }
    binary(&Value::Dictionary(d))
}

pub fn decode_ask(bytes: &[u8]) -> Result<AskRequest> {
    let d = dictionary(bytes)?;
    let files = d
        .get("Files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_dictionary)
        .map(|f| FileEntry {
            name: f
                .get("FileName")
                .and_then(Value::as_string)
                .unwrap_or("unnamed")
                .to_owned(),
            uti: f
                .get("FileType")
                .and_then(Value::as_string)
                .unwrap_or("public.data")
                .to_owned(),
            bom_path: f
                .get("FileBomPath")
                .and_then(Value::as_string)
                .unwrap_or("./unnamed")
                .to_owned(),
            is_directory: f
                .get("FileIsDirectory")
                .and_then(Value::as_boolean)
                .unwrap_or(false),
        })
        .collect();
    let items = d
        .get("Items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_string)
        .map(str::to_owned)
        .collect();
    Ok(AskRequest {
        sender_name: d
            .get("SenderComputerName")
            .and_then(Value::as_string)
            .unwrap_or("Unknown device")
            .to_owned(),
        sender_model: d
            .get("SenderModelName")
            .and_then(Value::as_string)
            .unwrap_or("Unknown")
            .to_owned(),
        sender_id: d
            .get("SenderID")
            .and_then(Value::as_string)
            .unwrap_or_default()
            .to_owned(),
        files,
        items,
        convert_media: d
            .get("ConvertMediaFormats")
            .and_then(Value::as_boolean)
            .unwrap_or(false),
        icon: d
            .get("FileIcon")
            .and_then(Value::as_data)
            .map(ToOwned::to_owned),
    })
}

pub fn extension_uti(path: &str) -> &'static str {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "public.jpeg",
        "png" => "public.png",
        "heic" | "heif" => "public.heic",
        "mov" => "com.apple.quicktime-movie",
        "mp4" => "public.mpeg-4",
        "pdf" => "com.adobe.pdf",
        "txt" => "public.plain-text",
        "zip" => "public.zip-archive",
        _ => "public.data",
    }
}

#[allow(dead_code)]
fn _ordered_map() -> BTreeMap<String, String> {
    BTreeMap::new()
}
