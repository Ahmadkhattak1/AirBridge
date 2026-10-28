use std::env;
use std::fs;
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use airbridge_core::{
    ArchiveEntry, AskRequest, FileEntry, HttpRequest, ReceiverEvent, ReceiverIdentity,
    ReceiverService, decode_ask, decode_discover_response, decode_gzip_cpio,
    encode_ask_for_transfer, encode_discover_request, encode_gzip_cpio, extension_uti,
    read_request_buffered_progress, read_response_buffered, write_request, write_response,
};
use anyhow::{Context, Result, bail};
use rcgen::generate_simple_self_signed;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, ServerConfig, ServerConnection,
    SignatureScheme, StreamOwned,
};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("receive") => receive(args.collect()),
        Some("send") => send(args.collect()),
        Some("choose-send") => choose_send(args.collect()),
        Some("inspect-ask") => {
            let path = args
                .next()
                .context("usage: airbridge inspect-ask <binary-plist>")?;
            let ask = decode_ask(&fs::read(&path).with_context(|| format!("read {path}"))?)?;
            println!("{}", serde_json::to_string_pretty(&ask)?);
            Ok(())
        }
        Some("inspect-upload") => {
            let path = args
                .next()
                .context("usage: airbridge inspect-upload <gzip-cpio>")?;
            for entry in decode_gzip_cpio(
                &fs::read(&path).with_context(|| format!("read {path}"))?,
                16 * 1024 * 1024 * 1024usize,
            )? {
                println!("{}\t{} bytes", entry.path, entry.data.len());
            }
            Ok(())
        }
        Some("version") => {
            println!("AirBridge {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => bail!("usage: airbridge <receive|send|inspect-ask|inspect-upload|version> ..."),
    }
}

#[derive(Debug)]
struct AirDropCertificateVerifier;

impl ServerCertVerifier for AirDropCertificateVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        // AirDrop peers intentionally use ephemeral self-signed identities.
        // Presence and user approval provide the trust decision for a transfer.
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
        ]
    }
}

fn send(args: Vec<String>) -> Result<()> {
    let target = option(&args, "--target")
        .context("usage: airbridge send --target '[IPv6%scope]:8770' [--name NAME] <file>...")?;
    let name = option(&args, "--name").unwrap_or_else(|| "Windows PC".into());
    let mut skip = false;
    let paths: Vec<PathBuf> = args
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            if skip {
                skip = false;
                return None;
            }
            if value == "--target" || value == "--name" {
                skip = true;
                return None;
            }
            if index > 0 && (args[index - 1] == "--target" || args[index - 1] == "--name") {
                return None;
            }
            Some(PathBuf::from(value))
        })
        .collect();
    if paths.is_empty() {
        bail!("select at least one file to send");
    }

    let mut files = Vec::new();
    let mut archive = Vec::new();
    let mut total_bytes = 0u64;
    for path in paths {
        let metadata = fs::metadata(&path).with_context(|| format!("read {}", path.display()))?;
        if !metadata.is_file() {
            bail!("folders are not supported yet: {}", path.display());
        }
        total_bytes = total_bytes.saturating_add(metadata.len());
        if total_bytes > 512 * 1024 * 1024 {
            bail!(
                "This test build supports up to 512 MiB per transfer. Select fewer or smaller files."
            );
        }
        let file_name = path
            .file_name()
            .and_then(|v| v.to_str())
            .context("file name is not valid Unicode")?
            .to_owned();
        files.push(FileEntry {
            name: file_name.clone(),
            uti: extension_uti(&file_name).into(),
            bom_path: format!("./{file_name}"),
            is_directory: false,
        });
        archive.push(ArchiveEntry {
            path: file_name,
            data: fs::read(&path)?,
            mode: 0o100644,
        });
    }

    let provider = rustls::crypto::ring::default_provider();
    let config = Arc::new(
        ClientConfig::builder_with_provider(provider.into())
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AirDropCertificateVerifier))
            .with_no_client_auth(),
    );
    let mut tls = connect_peer(&target, config.clone())?;
    let mut reader = BufReader::new(&mut tls);
    let host = target.clone();

    let discover = post("/Discover", &host, encode_discover_request()?);
    write_request(reader.get_mut(), &discover)?;
    let response =
        read_response_buffered(&mut reader, 8 * 1024 * 1024).context("read Discover response")?;
    if response.status != 200 {
        bail!("receiver discovery failed with HTTP {}", response.status);
    }
    let peer = decode_discover_response(&response.body)?;
    println!(
        "{{\"event\":\"peer\",\"name\":{}}}",
        serde_json::to_string(&peer.identity.computer_name)?
    );
    // Apple receivers expect discovery on its own connection, then Ask and
    // Upload together on a fresh connection.
    drop(reader);
    drop(tls);
    let mut tls = connect_peer(&target, config)?;
    let mut reader = BufReader::new(&mut tls);

    let ask = AskRequest {
        sender_name: name,
        sender_model: "Windows PC".into(),
        sender_id: fs::read_to_string("/sys/class/net/awdl0/address")
            .map(|mac| mac.trim().replace(':', ""))
            .unwrap_or_else(|_| format!("{:012x}", std::process::id())),
        files,
        items: Vec::new(),
        convert_media: false,
        icon: None,
    };
    let transfer_id = new_transfer_id()?;
    let sizes: Vec<_> = archive.iter().map(|file| file.data.len() as u64).collect();
    println!(
        "{}",
        serde_json::json!({"event":"asking", "transfer_id":transfer_id})
    );
    write_request(
        reader.get_mut(),
        &post(
            "/Ask",
            &host,
            encode_ask_for_transfer(&ask, &transfer_id, &sizes)?,
        ),
    )?;
    let response =
        read_response_buffered(&mut reader, 8 * 1024 * 1024).context("read Ask response")?;
    if response.status != 200 {
        bail!(
            "the receiver declined the transfer (HTTP {})",
            response.status
        );
    }

    println!(
        "{}",
        serde_json::json!({"event":"send-accepted", "transfer_id":transfer_id})
    );

    let mut upload = post("/Upload", &host, encode_gzip_cpio(&archive)?);
    upload.headers.insert("transferid".into(), transfer_id);
    upload.headers.insert("totalbytes".into(), "0".into());
    upload
        .headers
        .insert("content-type".into(), "application/x-cpio".into());
    println!(
        "{}",
        serde_json::json!({"event":"sending", "bytes":upload.body.len()})
    );
    write_request(reader.get_mut(), &upload).context("write Upload request")?;
    let response =
        read_response_buffered(&mut reader, 8 * 1024 * 1024).context("read Upload response")?;
    if response.status != 200 {
        bail!("upload failed with HTTP {}", response.status);
    }
    println!("{{\"event\":\"sent\",\"files\":{}}}", archive.len());
    Ok(())
}

fn choose_send(args: Vec<String>) -> Result<()> {
    use std::collections::{BTreeMap, HashMap};
    use std::net::{Ipv6Addr, SocketAddrV6};
    use std::time::{Duration, Instant};

    let radio_log = option(&args, "--radio-log").context("--radio-log is required")?;
    let selection = option(&args, "--selection-file").context("--selection-file is required")?;
    let name = option(&args, "--name").unwrap_or_else(|| "AirBridge".into());
    let mut paths = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if ["--radio-log", "--selection-file", "--name"].contains(&arg.as_str()) {
            iter.next();
        } else {
            paths.push(arg.clone());
        }
    }
    anyhow::ensure!(!paths.is_empty(), "select files to send");
    let scope: u32 = fs::read_to_string("/sys/class/net/awdl0/ifindex")?
        .trim()
        .parse()?;
    let config = Arc::new(
        ClientConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AirDropCertificateVerifier))
            .with_no_client_auth(),
    );
    let started = Instant::now();
    let mut last_try = HashMap::new();
    let mut choices: Vec<(String, String)> = Vec::new();
    loop {
        if started.elapsed() > Duration::from_secs(75) {
            bail!(
                "No receiver selected. Enable Everyone for 10 Minutes on your iPhone, keep it unlocked, and try again."
            );
        }
        if let Ok(chosen) = fs::read_to_string(&selection) {
            let chosen = chosen.trim();
            let target = choices
                .iter()
                .find(|(id, _)| id == chosen)
                .map(|(_, target)| target.clone())
                .context("the selected receiver is no longer available")?;
            fs::remove_file(&selection)?;
            let mut send_args = vec!["--target".into(), target, "--name".into(), name];
            send_args.extend(paths);
            return send(send_args);
        }
        let mut candidates = BTreeMap::new();
        for line in fs::read_to_string(&radio_log)?.lines() {
            let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let kind = event["event"].as_str().unwrap_or_default();
            if kind != "radio-peer" && kind != "receiver-found" {
                continue;
            }
            let Some(address) = event["address"]
                .as_str()
                .and_then(|s| s.parse::<Ipv6Addr>().ok())
            else {
                continue;
            };
            if !address.is_unicast_link_local() {
                continue;
            }
            // Radio-only fallback is a Discover request, never an automatic send.
            let port = event["port"]
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .unwrap_or(8770);
            if port != 0 {
                if kind == "receiver-found" {
                    candidates.insert(address, port);
                } else {
                    candidates.entry(address).or_insert(port);
                }
            }
        }
        for (address, port) in candidates.into_iter().take(16) {
            if started.elapsed() > Duration::from_secs(75) || Path::new(&selection).exists() {
                break;
            }
            let target = SocketAddrV6::new(address, port, 0, scope).to_string();
            if last_try
                .get(&target)
                .is_some_and(|last: &Instant| last.elapsed() < Duration::from_secs(5))
            {
                continue;
            }
            last_try.insert(target.clone(), Instant::now());
            // Re-discovery while the user chooses keeps the active session used.
            let result: Result<String> = (|| {
                let mut tls =
                    connect_peer_timeout(&target, config.clone(), Duration::from_secs(2))?;
                tls.sock.set_read_timeout(Some(Duration::from_secs(3)))?;
                tls.sock.set_write_timeout(Some(Duration::from_secs(3)))?;
                let mut reader = BufReader::new(&mut tls);
                write_request(
                    reader.get_mut(),
                    &post("/Discover", &target, encode_discover_request()?),
                )?;
                let response = read_response_buffered(&mut reader, 8 * 1024 * 1024)?;
                anyhow::ensure!(response.status == 200, "discovery was declined");
                Ok(decode_discover_response(&response.body)?
                    .identity
                    .computer_name)
            })();
            if let Ok(peer_name) = result {
                if !choices.iter().any(|(_, existing)| existing == &target) {
                    let id = (choices.len() + 1).to_string();
                    choices.push((id.clone(), target));
                    println!(
                        "{}",
                        serde_json::json!({"event":"send-target", "id":id, "name":peer_name})
                    );
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn new_transfer_id() -> Result<String> {
    let mut bytes = [0; 16];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("could not generate a transfer identifier"))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

fn post(path: &str, host: &str, body: Vec<u8>) -> HttpRequest {
    HttpRequest {
        method: "POST".into(),
        path: path.into(),
        headers: std::collections::BTreeMap::from([
            ("host".into(), host.into()),
            ("user-agent".into(), "AirDrop/1.0".into()),
            ("content-type".into(), "application/octet-stream".into()),
            ("connection".into(), "keep-alive".into()),
        ]),
        body,
    }
}

fn connect_peer(
    target: &str,
    config: Arc<ClientConfig>,
) -> Result<StreamOwned<ClientConnection, TcpStream>> {
    connect_peer_timeout(target, config, std::time::Duration::from_secs(10))
}

fn connect_peer_timeout(
    target: &str,
    config: Arc<ClientConfig>,
    timeout: std::time::Duration,
) -> Result<StreamOwned<ClientConnection, TcpStream>> {
    let address = target
        .to_socket_addrs()?
        .next()
        .context("target did not resolve")?;
    let tcp = TcpStream::connect_timeout(&address, timeout)?;
    tcp.set_nodelay(true)?;
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
    tcp.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let connection =
        ClientConnection::new(config, ServerName::try_from("airdrop.local")?.to_owned())?;
    Ok(StreamOwned::new(connection, tcp))
}

fn receive(args: Vec<String>) -> Result<()> {
    let listen = option(&args, "--listen").unwrap_or_else(|| "[::]:8770".into());
    let directory = PathBuf::from(option(&args, "--dir").unwrap_or_else(|| "received".into()));
    let name = option(&args, "--name").unwrap_or_else(|| "AirBridge".into());
    let approval_file = option(&args, "--approval-file").map(PathBuf::from);
    let yes = args.iter().any(|a| a == "--yes");
    fs::create_dir_all(&directory)?;

    let certified = generate_simple_self_signed(vec![name.clone(), "airbridge.local".into()])?;
    let cert = CertificateDer::from(certified.cert.der().to_vec());
    let key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
    let tls = Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key.into())?,
    );
    let listener = TcpListener::bind(&listen).with_context(|| format!("listen on {listen}"))?;
    let identity = ReceiverIdentity {
        computer_name: name.clone(),
        model_name: "Windows PC".into(),
    };
    // AirDrop is free to use a fresh TLS connection for /Upload.  Keep the
    // receiver state at listener scope so an accepted /Ask survives that
    // connection boundary.
    let mut services = std::collections::HashMap::new();
    println!(
        "{{\"event\":\"ready\",\"name\":{}}}",
        serde_json::to_string(&name)?
    );

    for connection in listener.incoming() {
        match connection {
            Ok(tcp) => {
                let tls = tls.clone();
                let peer = tcp.peer_addr()?.ip();
                if services.len() >= 64 && !services.contains_key(&peer) {
                    continue;
                }
                let service = services
                    .entry(peer)
                    .or_insert_with(|| {
                        Arc::new(Mutex::new(ReceiverService::new(
                            identity.clone(),
                            512 * 1024 * 1024,
                        )))
                    })
                    .clone();
                let directory = directory.clone();
                let approval_file = approval_file.clone();
                std::thread::spawn(move || {
                    if let Err(error) =
                        serve(tcp, tls, service, &directory, yes, approval_file.as_deref())
                    {
                        let message = format!("{error:#}");
                        if !message.contains("close_notify")
                            && !message.contains("connection closed")
                            && (!message.contains("Connection reset by peer")
                                || message.contains("file transfer stopped"))
                        {
                            println!(
                                "{}",
                                serde_json::json!({"event":"transfer-error", "message":message})
                            );
                        }
                    }
                });
            }
            Err(error) => eprintln!("accept: {error}"),
        }
    }
    Ok(())
}

fn serve(
    tcp: TcpStream,
    config: Arc<ServerConfig>,
    service: Arc<Mutex<ReceiverService>>,
    directory: &Path,
    yes: bool,
    approval_file: Option<&Path>,
) -> Result<()> {
    tcp.set_nodelay(true)?;
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
    tcp.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let connection = ServerConnection::new(config)?;
    let mut tls = StreamOwned::new(connection, tcp);
    let mut reader = BufReader::new(&mut tls);
    let mut awaiting_upload = false;
    loop {
        let mut progress_at = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let request =
            match read_request_buffered_progress(&mut reader, 512 * 1024 * 1024, |bytes, total| {
                if progress_at.elapsed() >= std::time::Duration::from_millis(500) {
                    println!(
                        "{}",
                        serde_json::json!({"event":"receiving", "bytes":bytes, "total":total})
                    );
                    progress_at = std::time::Instant::now();
                }
            }) {
                Ok(request) => request,
                Err(airbridge_core::Error::Http(message)) if message == "connection closed" => {
                    return Ok(());
                }
                Err(error) => {
                    return Err(if awaiting_upload {
                        anyhow::Error::from(error).context("The file transfer stopped")
                    } else {
                        error.into()
                    });
                }
            };
        println!(
            "{}",
            serde_json::json!({"event":"request", "path":request.path, "bytes":request.body.len(), "type":request.headers.get("content-type")})
        );
        let (response, events) = service.lock().expect("receiver state poisoned").handle(
            request,
            |ask| {
                if yes {
                    return true;
                }
                if let Some(path) = approval_file {
                    let _ = fs::remove_file(path);
                    println!(
                        "{{\"event\":\"approval-needed\",\"sender\":{},\"files\":{},\"links\":{}}}",
                        serde_json::to_string(&ask.sender_name)
                            .unwrap_or_else(|_| "\"iPhone\"".into()),
                        ask.files.len(),
                        ask.items.len()
                    );
                    let _ = std::io::stdout().flush();
                    for _ in 0..1200 {
                        if let Ok(answer) = fs::read_to_string(path) {
                            let _ = fs::remove_file(path);
                            return answer.trim().eq_ignore_ascii_case("accept");
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    return false;
                }
                eprintln!(
                    "{} wants to send {} file(s) and {} link(s). Accept? [y/N]",
                    ask.sender_name,
                    ask.files.len(),
                    ask.items.len()
                );
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer).is_ok()
                    && answer.trim().eq_ignore_ascii_case("y")
            },
        )?;
        for event in events {
            match event {
                ReceiverEvent::ConsentRequested(ask) => println!(
                    "{{\"event\":\"ask\",\"sender\":{},\"files\":{},\"links\":{}}}",
                    serde_json::to_string(&ask.sender_name)?,
                    ask.files.len(),
                    ask.items.len()
                ),
                ReceiverEvent::FilesReceived(files) => {
                    awaiting_upload = false;
                    let mut saved = Vec::new();
                    for file in files {
                        let target = unique_path(directory, &file.path);
                        let is_directory = file.mode & 0o170000 == 0o040000;
                        if is_directory {
                            fs::create_dir_all(&target)?;
                            continue;
                        } else if let Some(parent) = target.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(&target, file.data)?;
                        saved.push(target.display().to_string());
                    }
                    println!(
                        "{{\"event\":\"received\",\"files\":{}}}",
                        serde_json::to_string(&saved)?
                    );
                }
                ReceiverEvent::LinksReceived(links) => println!(
                    "{{\"event\":\"links\",\"items\":{}}}",
                    serde_json::to_string(&links)?
                ),
                ReceiverEvent::Discovered => println!("{{\"event\":\"discovered\"}}"),
                ReceiverEvent::Accepted(_) => {
                    awaiting_upload = true;
                    println!("{{\"event\":\"accepted\"}}");
                }
                ReceiverEvent::Declined => println!("{{\"event\":\"declined\"}}"),
            }
        }
        write_response(reader.get_mut(), &response)?;
        reader.get_mut().flush()?;
        if response
            .headers
            .get("connection")
            .is_some_and(|v| v == "close")
        {
            return Ok(());
        }
    }
}

fn unique_path(root: &Path, relative: &str) -> PathBuf {
    let requested = root.join(relative);
    if !requested.exists() {
        return requested;
    }
    let parent = requested.parent().unwrap_or(root);
    let stem = requested
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = requested
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{s}"))
        .unwrap_or_default();
    for n in 2..10000 {
        let candidate = parent.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-copy{ext}"))
}

fn option(args: &[String], key: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}
