#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The AirBridge radio runs inside AirBridgeRuntime.");
    std::process::exit(1);
}
#[cfg(target_os = "linux")]
mod quic_discovery;
