//! HTTP/3 on the receiver's AWDL address. Each QUIC connection owns its
//! consent state so concurrent streams cannot borrow another peer's approval.
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use airbridge_core::{HttpRequest, HttpResponse, ReceiverIdentity, ReceiverService};
use anyhow::{Context, Result, ensure};
use bytes::{Buf, Bytes};
use quinn::crypto::rustls::QuicServerConfig;
use tokio::{sync::mpsc, task::JoinSet, time::timeout};

type Stream = h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;
const IDLE: Duration = Duration::from_secs(120);
const CONTROL_LIMIT: usize = 8 * 1024 * 1024;

pub struct Server {
    endpoint: quinn::Endpoint,
    _runtime: tokio::runtime::Runtime,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.endpoint.close(0u32.into(), b"receiver stopped");
    }
}

pub fn start(
    address: SocketAddr,
    mut tls: rustls::ServerConfig,
    identity: ReceiverIdentity,
    directory: PathBuf,
    yes: bool,
    approval: Option<PathBuf>,
) -> Result<Server> {
    tls.alpn_protocols = vec![b"h3".to_vec()];
    tls.max_early_data_size = 0;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(tls)?));
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(8u32.into())
        .max_concurrent_uni_streams(8u32.into())
        .stream_receive_window((4 * 1024 * 1024u32).into())
        .receive_window((16 * 1024 * 1024u32).into())
        .max_idle_timeout(Some(IDLE.try_into()?));
    config.transport_config(Arc::new(transport));
    config.migration(false);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _entered = runtime.enter();
    let endpoint = quinn::Endpoint::server(config, address)?;
    let listener = endpoint.clone();
    runtime.spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                incoming = listener.accept() => {
                    let Some(incoming) = incoming else { break };
                    if connections.len() >= 8 {
                        incoming.refuse();
                        continue;
                    }
                    let identity = identity.clone();
                    let directory = directory.clone();
                    let approval = approval.clone();
                    connections.spawn(async move {
                        let result = async {
                            let connection = timeout(Duration::from_secs(15), incoming).await??;
                            println!("{}", serde_json::json!({"event":"http3-connected", "peer":connection.remote_address().to_string()}));
                            serve(connection, identity, directory, yes, approval).await
                        }.await;
                        if let Err(error) = result {
                            eprintln!("HTTP/3 connection ended: {error:#}");
                        }
                    });
                }
                _ = connections.join_next(), if !connections.is_empty() => {}
            }
        }
    });
    drop(_entered);
    println!(
        "{}",
        serde_json::json!({"event":"http3-ready", "address":endpoint.local_addr()?.to_string()})
    );
    Ok(Server {
        endpoint,
        _runtime: runtime,
    })
}

async fn serve(
    connection: quinn::Connection,
    identity: ReceiverIdentity,
    directory: PathBuf,
    yes: bool,
    approval: Option<PathBuf>,
) -> Result<()> {
    let service = Arc::new(Mutex::new(ReceiverService::new(
        identity,
        super::incoming::MAX_UPLOAD,
    )));
    let mut http = h3::server::builder()
        .max_field_section_size(32 * 1024)
        .build(h3_quinn::Connection::new(connection))
        .await?;
    let mut requests = JoinSet::new();
    loop {
        tokio::select! {
            next = http.accept() => {
                let Some(resolver) = next? else { break };
                let (request, stream) = timeout(IDLE, resolver.resolve_request()).await??;
                if requests.len() >= 16 {
                    let mut stream = stream;
                    respond(&mut stream, status(503)).await?;
                    continue;
                }
                let service = service.clone();
                let directory = directory.clone();
                let approval = approval.clone();
                requests.spawn(async move {
                    if let Err(error) = handle(request, stream, service, directory, yes, approval).await {
                        println!("{}", serde_json::json!({"event":"transfer-error", "message":format!("HTTP/3 transfer stopped: {error:#}")}));
                    }
                });
            }
            _ = requests.join_next(), if !requests.is_empty() => {}
        }
    }
    requests.abort_all();
    Ok(())
}

async fn respond(stream: &mut Stream, response: HttpResponse) -> Result<()> {
    let mut builder = http::Response::builder()
        .status(response.status)
        .header("content-length", response.body.len());
    for (key, value) in response.headers {
        if !matches!(
            key.as_str(),
            "connection" | "transfer-encoding" | "content-length"
        ) {
            builder = builder.header(key, value);
        }
    }
    stream.send_response(builder.body(())?).await?;
    if !response.body.is_empty() {
        stream.send_data(Bytes::from(response.body)).await?;
    }
    stream.finish().await?;
    Ok(())
}

fn status(code: u16) -> HttpResponse {
    HttpResponse {
        status: code,
        reason: String::new(),
        headers: Default::default(),
        body: Vec::new(),
    }
}

async fn handle(
    request: http::Request<()>,
    mut stream: Stream,
    service: Arc<Mutex<ReceiverService>>,
    directory: PathBuf,
    yes: bool,
    approval: Option<PathBuf>,
) -> Result<()> {
    let mut request = HttpRequest {
        method: request.method().to_string(),
        path: request.uri().path().into(),
        headers: request
            .headers()
            .iter()
            .map(|(name, value)| Ok((name.as_str().to_owned(), value.to_str()?.to_owned())))
            .collect::<Result<_>>()?,
        body: Vec::new(),
    };
    println!(
        "{}",
        serde_json::json!({"event":"http3-request", "path":request.path,
        "content_type":request.headers.get("content-type"), "stream":stream.id().to_string()})
    );
    if request.method != "POST" {
        return respond(&mut stream, status(405)).await;
    }
    if !matches!(
        request.path.as_str(),
        "/Hello" | "/Discover" | "/Ask" | "/Upload"
    ) {
        return respond(&mut stream, status(404)).await;
    }
    if request.path == "/Upload" {
        return upload(stream, request, service, directory).await;
    }
    while let Some(mut chunk) = timeout(IDLE, stream.recv_data()).await?? {
        if request.body.len() + chunk.remaining() > CONTROL_LIMIT {
            return respond(&mut stream, status(413)).await;
        }
        request
            .body
            .extend_from_slice(&chunk.copy_to_bytes(chunk.remaining()));
    }
    let response = if request.path == "/Hello" {
        match hello(&request.body) {
            Ok(response) => response,
            Err(error) => {
                eprintln!("HTTP/3 Hello rejected: {error:#}");
                status(400)
            }
        }
    } else {
        tokio::task::spawn_blocking(move || {
            super::handle_control(request, &service, &directory, yes, approval.as_deref())
        })
        .await??
    };
    respond(&mut stream, response).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use airbridge_core::{
        ArchiveEntry, AskRequest, FileEntry, encode_ask_for_transfer, encode_gzip_cpio,
    };
    use rustls::pki_types::PrivatePkcs8KeyDer;

    type Client = h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>;

    async fn connect(endpoint: &quinn::Endpoint, address: SocketAddr) -> Result<Client> {
        let connection = endpoint
            .connect(address, "airbridge.local")
            .context("start QUIC connection")?
            .await
            .context("QUIC handshake")?;
        let (mut driver, sender) = h3::client::new(h3_quinn::Connection::new(connection)).await?;
        tokio::spawn(async move {
            std::future::poll_fn(|cx| driver.poll_close(cx)).await;
        });
        Ok(sender)
    }

    async fn request(
        client: &mut Client,
        path: &str,
        body: Vec<u8>,
        finish: bool,
    ) -> Result<(u16, Vec<u8>)> {
        let mut stream = client
            .send_request(
                http::Request::post(format!("https://airbridge.local{path}"))
                    .header("transferid", "http3-test")
                    .header(
                        "content-type",
                        if path == "/Upload" {
                            "application/x-cpio"
                        } else {
                            "application/octet-stream"
                        },
                    )
                    .body(())?,
            )
            .await?;
        for chunk in body.chunks(32 * 1024) {
            stream.send_data(Bytes::copy_from_slice(chunk)).await?;
        }
        if finish {
            stream.finish().await?;
        }
        let response = timeout(Duration::from_secs(10), stream.recv_response()).await??;
        let mut bytes = Vec::new();
        while let Some(mut chunk) = stream.recv_data().await? {
            bytes.extend_from_slice(&chunk.copy_to_bytes(chunk.remaining()));
        }
        Ok((response.status().as_u16(), bytes))
    }

    #[test]
    fn http3_streams_require_consent_preserve_files_and_finish_without_sender_fin() -> Result<()> {
        let certificate = rcgen::generate_simple_self_signed(vec!["airbridge.local".into()])?;
        let der = certificate.cert.der().clone();
        let tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![der.clone()],
                PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der()).into(),
            )?;
        let destination = tempfile::tempdir()?;
        let server = start(
            "[::1]:0".parse()?,
            tls,
            ReceiverIdentity {
                computer_name: "PC".into(),
                model_name: "Windows".into(),
            },
            destination.path().into(),
            true,
            None,
        )?;
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            let mut roots = rustls::RootCertStore::empty();
            roots.add(der)?;
            let mut tls = rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            tls.alpn_protocols = vec![b"h3".to_vec()];
            let mut endpoint =
                quinn::Endpoint::client("[::]:0".parse()?).context("bind QUIC client")?;
            endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(
                quinn::crypto::rustls::QuicClientConfig::try_from(tls)?,
            )));
            let mut client = connect(&endpoint, server.endpoint.local_addr()?).await?;
            assert_eq!(
                request(&mut client, "/Upload", Vec::new(), true).await?.0,
                409
            );
            assert_eq!(
                request(&mut client, "/Hello", b"bad".to_vec(), true)
                    .await?
                    .0,
                400
            );
            let (status, discover) = request(&mut client, "/Discover", Vec::new(), true).await?;
            assert_eq!(status, 200);
            let capabilities = airbridge_core::decode_discover_response(&discover)?;
            assert_eq!(capabilities.identity.computer_name, "PC");
            let mut hello = Vec::new();
            plist::to_writer_binary(
                &mut hello,
                &plist::Dictionary::from_iter([("featureFlags", plist::Value::from(0u64))]),
            )?;
            assert_eq!(request(&mut client, "/Hello", hello, true).await?.0, 200);
            let ask = AskRequest {
                sender_name: "Phone".into(),
                sender_model: "iPhone".into(),
                sender_id: "phone".into(),
                items: vec![],
                convert_media: false,
                icon: None,
                files: vec![FileEntry {
                    name: "original.mov".into(),
                    bom_path: "./original.mov".into(),
                    uti: "com.apple.quicktime-movie".into(),
                    is_directory: false,
                }],
            };
            let ask = encode_ask_for_transfer(&ask, "http3-test", &[2 * 1024 * 1024])?;
            assert_eq!(
                request(&mut client, "/Ask", ask.clone(), true).await?.0,
                200
            );
            // A second encrypted connection must obtain its own approval.
            let mut other = connect(&endpoint, server.endpoint.local_addr()?).await?;
            assert_eq!(
                request(&mut other, "/Upload", Vec::new(), true).await?.0,
                409
            );
            let data: Vec<u8> = (0..2 * 1024 * 1024)
                .map(|i| (i * 97 + i / 251) as u8)
                .collect();
            let archive = encode_gzip_cpio(&[ArchiveEntry {
                path: "original.mov".into(),
                mode: 0o100644,
                data: data.clone(),
            }])?;
            assert_eq!(
                request(&mut client, "/Upload", archive, false).await?.0,
                200
            );
            assert_eq!(
                std::fs::read(destination.path().join("original.mov"))?,
                data
            );
            assert_eq!(
                request(&mut client, "/Upload", Vec::new(), true).await?.0,
                409
            );
            assert_eq!(request(&mut client, "/Ask", ask, true).await?.0, 200);
            let result = request(&mut client, "/Upload", b"bad".to_vec(), true).await;
            assert!(result.is_err() || result.as_ref().unwrap().0 != 200);
            assert_eq!(std::fs::read_dir(destination.path())?.count(), 1);
            endpoint.close(0u32.into(), b"test complete");
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(())
    }
}

fn hello(body: &[u8]) -> Result<HttpResponse> {
    // The native /Hello handler uses PropertyListDecoder. The CodingKeys are
    // recovered from sharingd; the phone's actual exchange remains a device test.
    let value = plist::Value::from_reader(std::io::Cursor::new(body))?;
    let request = value
        .as_dictionary()
        .context("Hello must be a dictionary")?;
    ensure!(
        request.contains_key("featureFlags"),
        "Hello is missing featureFlags"
    );
    let mut dictionary = plist::Dictionary::new();
    dictionary.insert("featureFlags".into(), 0u64.into());
    let mut response = status(200);
    plist::to_writer_binary(&mut response.body, &dictionary)?;
    response
        .headers
        .insert("content-type".into(), "application/octet-stream".into());
    Ok(response)
}

async fn upload(
    mut stream: Stream,
    request: HttpRequest,
    service: Arc<Mutex<ReceiverService>>,
    directory: PathBuf,
) -> Result<()> {
    // Bounded chunks keep filesystem work off the QUIC executor and apply
    // backpressure without holding the entire movie in memory.
    let (send, mut receive) = mpsc::channel::<Bytes>(8);
    let mut writer = tokio::task::spawn_blocking(move || {
        super::incoming::receive_upload_from(&request, service, &directory, |sink| {
            while let Some(bytes) = receive.blocking_recv() {
                if sink(&bytes)?.is_break() {
                    return Ok(());
                }
            }
            Err(airbridge_core::Error::Http(
                "upload stream ended before archive completion".into(),
            ))
        })
    });
    loop {
        tokio::select! {
            result = &mut writer => {
                return respond(&mut stream, result??).await;
            }
            chunk = timeout(IDLE, stream.recv_data()) => {
                let Some(mut chunk) = chunk?? else {
                    drop(send);
                    return respond(&mut stream, writer.await??).await;
                };
                while chunk.has_remaining() {
                    let bytes = chunk.copy_to_bytes(chunk.remaining().min(64 * 1024));
                    if send.send(bytes).await.is_err() {
                        return respond(&mut stream, writer.await??).await;
                    }
                }
            }
        }
    }
}
