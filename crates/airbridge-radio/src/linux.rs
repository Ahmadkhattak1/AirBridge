use std::io::Write;
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use owly_airdrop::proto::mdns::{self, Announcer, BrowseEvent, Browser, Burst, Message, Service};
use owly_awdl::{Channel, Config, MacAddr, Presence, RadioConstraints, TxRate};
use owly_radio::native::Native;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::{net::UdpSocket, sync::watch};

pub fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(start())
}

async fn start() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let option = |key: &str| args.windows(2).find(|a| a[0] == key).map(|a| a[1].clone());
    let monitor = option("--monitor").context("--monitor is required")?;
    if args.iter().any(|arg| arg == "--wake") {
        return wake_iphone(&monitor).await;
    }
    let name = option("--name").unwrap_or_else(|| "AirBridge".into());
    let port: u16 = option("--port").unwrap_or_else(|| "8770".into()).parse()?;
    let browse = args.iter().any(|arg| arg == "--browse");
    let channel = option("--channel")
        .unwrap_or_else(|| "6".into())
        .parse::<u8>()?;
    anyhow::ensure!([6, 44, 149].contains(&channel), "unsupported AWDL channel");
    let rx_streams = option("--rx-streams")
        .unwrap_or_else(|| "1".into())
        .parse::<u8>()?;
    anyhow::ensure!(
        (1..=2).contains(&rx_streams),
        "unsupported receive stream count"
    );
    let mac: MacAddr = std::fs::read_to_string(format!("/sys/class/net/{monitor}/address"))?
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut cfg = Config::new(
        mac,
        &name,
        RadioConstraints {
            channel: Channel(channel),
            presence: Presence::Fixed,
        },
    );
    // The runtime supplies the adapter's transmit rate and receive streams.
    // The library checks the peer's MCS support before sending HT frames.
    let rate = option("--tx-rate").unwrap_or_else(|| "mcs7".into());
    cfg.rate = TxRate::parse(&rate).context("invalid --tx-rate")?;
    cfg.ht.rx_mcs = (1u32 << (rx_streams * 8)) - 1;
    cfg.amsdu = true;
    cfg.rx_reply = args.iter().any(|arg| arg == "--rx-reply");
    // Opt in only on radio setups that supply hardware Block Ack reception.
    cfg.accept_addba = args.iter().any(|arg| arg == "--rx-ampdu");
    println!(
        "{}",
        serde_json::json!({"event":"radio-config", "tx_rate":cfg.rate.to_string(),
        "tx_amsdu":cfg.amsdu, "rx_ampdu":cfg.accept_addba, "channel":channel,
        "rx_streams":rx_streams, "rx_reply":cfg.rx_reply})
    );
    let (peer_tx, mut peers) = watch::channel(Vec::new());
    let node = Native::start(&monitor, "awdl0", cfg, peer_tx)?;
    let scope = node.index;
    let mut failed = node.failed();
    let socket = socket(scope)?;
    let service = Service {
        instance: mac.0.iter().map(|b| format!("{b:02x}")).collect(),
        host: "airbridge".into(),
        port,
        addr: mac.link_local(),
        // Discover + mixed files + DVZip (verified with iPhone uploads).
        // Pipelined uploads are not advertised.
        flags: 0x8a,
    };
    let group = SocketAddrV6::new(mdns::MDNS_GROUP, mdns::MDNS_PORT, 0, scope);
    let announcement = service.announcement();
    let quic = args.iter().any(|arg| arg == "--http3").then(|| {
        // The captured Application Service uses a UUID-shaped instance and
        // an empty TXT record, independently of the legacy AirDrop instance.
        let instance = format!("00000000-0000-4000-8000-{}", service.instance);
        crate::quic_discovery::QuicService::new(&service, &instance)
    });
    let mut announcer = Announcer::new(
        Some(Burst {
            count: 64,
            interval: Duration::from_micros(16_400),
            pause: Duration::from_secs(3),
        }),
        Instant::now(),
    );
    let mut buf = vec![0; mdns::MAX_MESSAGE];
    let mut browser = Browser::new(Instant::now());
    let mut browse_tick = tokio::time::interval(Duration::from_millis(100));
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    if let Some(path) = option("--ready-file") {
        std::fs::write(path, scope.to_string())?;
    }
    println!(
        "{}",
        serde_json::json!({"event":"radio-ready", "interface":monitor, "scope":scope})
    );
    let result: Result<()> = async {
        loop {
            let wait = announcer.next_timer().unwrap().saturating_duration_since(Instant::now());
            tokio::select! {
                _ = terminate.recv() => break,
                _ = interrupt.recv() => break,
                _ = failed.changed() => bail!("radio stopped: {:?}", *failed.borrow()),
                changed = peers.changed() => {
                    changed?;
                    println!("{}", serde_json::json!({"event":"radio-peers", "count":peers.borrow().len()}));
                    if browse {
                        for peer in peers.borrow().iter() {
                            println!("{}", serde_json::json!({"event":"radio-peer", "mac":peer.mac.to_string(),
                                "address":peer.mac.link_local().to_string(), "name":peer.name, "rssi":peer.rssi}));
                        }
                    }
                }
                _ = browse_tick.tick(), if browse => {
                    if let Some(query) = browser.on_timer(Instant::now()) {
                        socket.send_to(&query, group).await?;
                        // Query known AWDL peers directly as well. Their multicast
                        // listening windows can be much shorter than ours.
                        let addresses: Vec<_> = peers.borrow().iter().map(|p| p.mac.link_local()).collect();
                        for address in addresses {
                            let target = SocketAddrV6::new(address, mdns::MDNS_PORT, 0, scope);
                            let _ = socket.send_to(&query, target).await;
                        }
                        println!("{}", serde_json::json!({"event":"receiver-query"}));
                    }
                    report_browser(&mut browser, mac.link_local(), scope);
                }
                _ = tokio::time::sleep(wait) => {
                    if announcer.on_timer(Instant::now()) {
                        if port != 0 {
                            socket.send_to(&announcement, group).await.context("announce AirBridge")?;
                            if let Some(quic) = &quic {
                                socket.send_to(&quic.announcement(false), group).await?;
                            }
                        }
                        if browse {
                            let query = Message { questions: vec![mdns::Question {
                                name: mdns::Name::service(), qtype: mdns::rtype::PTR, unicast: false,
                            }], ..Message::default() }.encode();
                            socket.send_to(&query, group).await?;
                        }
                    }
                }
                received = socket.recv_from(&mut buf) => {
                    let (n, from) = received?;
                    let SocketAddr::V6(from) = from else { continue };
                    if from.scope_id() != scope { continue; }
                    let Ok(query) = Message::decode(&buf[..n]) else { continue };
                    if browse && *from.ip() != mac.link_local() {
                        println!("{}", serde_json::json!({"event":"mdns-packet", "from":from.to_string(),
                            "response":query.response, "questions":query.questions.len(),
                            "answers":query.answers.len(), "additional":query.additional.len(),
                            "records":format!("{:?} {:?}", query.answers, query.additional)}));
                        browser.handle(&query, Instant::now());
                        report_browser(&mut browser, mac.link_local(), scope);
                    }
                    if port == 0 { continue; }
                    if let Some(quic) = &quic
                        && let Some(reply) = quic.answer(&query, from.port() != mdns::MDNS_PORT) {
                        socket.send_to(&reply.message, if reply.unicast {from} else {group}).await?;
                    }
                    if let Some(reply) = service.answer(&query, from.port() != mdns::MDNS_PORT) {
                        println!("{}", serde_json::json!({"event":"discovery-query", "peer":from.ip().to_string()}));
                        socket.send_to(&reply.message, if reply.unicast {from} else {group}).await?;
                    }
                }
            }
        }
        Ok(())
    }.await;
    if port != 0 {
        let _ = socket.send_to(&service.goodbye(), group).await;
        if let Some(quic) = &quic {
            let _ = socket.send_to(&quic.announcement(true), group).await;
        }
    }
    node.stop();
    result
}

fn report_browser(browser: &mut Browser, own: Ipv6Addr, scope: u32) {
    while let Some(event) = browser.poll_event() {
        match event {
            BrowseEvent::Found(peer) if peer.addr != own => println!(
                "{}",
                serde_json::json!({
                    "event":"receiver-found", "instance":peer.instance, "host":peer.host,
                    "address":peer.addr.to_string(), "port":peer.port, "flags":peer.flags,
                    "target":SocketAddrV6::new(peer.addr, peer.port, 0, scope).to_string(),
                })
            ),
            BrowseEvent::Lost(instance) => println!(
                "{}",
                serde_json::json!({"event":"receiver-lost", "instance":instance})
            ),
            _ => {}
        }
    }
}

async fn wake_iphone(monitor: &str) -> Result<()> {
    // Generated by experiments/wifi_ble, independently checked against the
    // Bluetooth SIG framing vector. This exact waveform reached the test iPhone.
    const FRAMES: &[u8] = include_bytes!("../assets/cck-airdrop-ch38.bin");
    const FRAME_BYTES: usize = 561;
    anyhow::ensure!(
        FRAMES.len() == 128 * FRAME_BYTES,
        "invalid wake frame asset"
    );
    anyhow::ensure!(
        monitor == "abcck0",
        "wake needs the isolated abcck0 monitor"
    );
    anyhow::ensure!(
        std::fs::read_to_string("/sys/module/rtl8xxxu/parameters/airbridge_cck")?.trim() == "Y",
        "the opt-in CCK driver is not enabled"
    );
    let index: u32 = std::fs::read_to_string(format!("/sys/class/net/{monitor}/ifindex"))?
        .trim()
        .parse()?;
    let mac: MacAddr = std::fs::read_to_string(format!("/sys/class/net/{monitor}/address"))?
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut file = std::fs::File::from(owly_radio::native::packet_socket(index, false)?);
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let started = Instant::now();
    let mut sent = 0usize;
    println!("{}", serde_json::json!({"event":"waking", "seconds":20}));
    while started.elapsed() < Duration::from_secs(20) {
        let at = (sent % 128) * FRAME_BYTES;
        let mut frame = FRAMES[at..at + FRAME_BYTES].to_vec();
        frame[22..28].copy_from_slice(&mac.0);
        match file.write(&frame) {
            Ok(n) if n == FRAME_BYTES => sent += 1,
            Ok(_) => bail!("partial wake frame write"),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        tokio::select! {
            _ = terminate.recv() => bail!("wake cancelled"),
            _ = interrupt.recv() => bail!("wake cancelled"),
            _ = tokio::time::sleep(Duration::from_millis(5)) => {},
        }
    }
    println!(
        "{}",
        serde_json::json!({"event":"wake-sent", "packets":sent})
    );
    Ok(())
}

fn socket(scope: u32) -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_only_v6(true)?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    // Restrict both received and sent traffic to this AWDL interface.
    socket.bind_device(Some(b"awdl0"))?;
    socket.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, mdns::MDNS_PORT)).into())?;
    socket.join_multicast_v6(&mdns::MDNS_GROUP, scope)?;
    socket.set_multicast_if_v6(scope)?;
    socket.set_multicast_hops_v6(255)?;
    socket.set_multicast_loop_v6(false)?;
    socket.set_nonblocking(true)?;
    Ok(UdpSocket::from_std(socket.into())?)
}
