use super::{AirBridgeApp, Event, bounded_output, hardware, hidden_command, wsl_command};
use std::process::{Child, Stdio};
use std::sync::atomic::Ordering;
use std::time::Duration;

// Keeping stdin open keeps the distro alive until the app closes. A fixed sleep
// can expire while the user is choosing files and silently detach the adapter.
struct RuntimeLease(Child);

impl RuntimeLease {
    fn start() -> Result<Self, String> {
        wsl_command()
            .arg("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(Self)
            .map_err(|error| format!("Could not start the radio runtime: {error}"))
    }

    fn running(&mut self) -> bool {
        self.0.try_wait().is_ok_and(|status| status.is_none())
    }
}

impl Drop for RuntimeLease {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl AirBridgeApp {
    pub fn watch_connection(&mut self) {
        self.hardware_checking = true;
        let tx = self.events_tx.clone();
        let shutdown = self.shutdown.clone();
        let operation = self.operation.clone();
        std::thread::spawn(move || {
            let mut lease: Option<RuntimeLease> = None;
            let mut was_ready = false;
            while !shutdown.load(Ordering::SeqCst) {
                // A live transfer owns the radio. Never probe/reconfigure it
                // halfway through receiving, or compete with its cleanup.
                if let Ok(_guard) = operation.try_lock() {
                    let mut report = hardware::probe();
                    if report.ready && !shutdown.load(Ordering::SeqCst) {
                        if !was_ready {
                            let _ = tx.send(Event::Connecting);
                        }
                        if !lease.as_mut().is_some_and(RuntimeLease::running) {
                            lease = None;
                        }
                        let result = (|| {
                            if lease.is_none() {
                                let _ = tx.send(Event::Connecting);
                                lease = Some(RuntimeLease::start()?);
                                let output = bounded_output(
                                    wsl_command().arg("true"),
                                    Duration::from_secs(20),
                                )
                                .map_err(|error| {
                                    format!("The radio runtime did not start: {error}")
                                })?;
                                if !output.status.success() {
                                    return Err("The radio runtime could not start.".into());
                                }
                            }
                            attach_adapter()?;
                            let output = bounded_output(
                                wsl_command().args([
                                    "bash",
                                    "/opt/airbridge/bin/airbridge-radio",
                                    "prepare",
                                ]),
                                Duration::from_secs(30),
                            )
                            .map_err(|error| format!("Adapter setup timed out: {error}"))?;
                            if !output.status.success()
                                || !String::from_utf8_lossy(&output.stdout)
                                    .contains("\"event\":\"adapter-ready\"")
                            {
                                return Err(String::from_utf8_lossy(&output.stderr)
                                    .trim()
                                    .to_owned());
                            }
                            Ok(())
                        })();
                        match result {
                            Ok(()) => report.summary = "Ready to send and receive".into(),
                            Err(error) => {
                                report.ready = false;
                                report.summary =
                                    "Could not connect the adapter. Unplug it, then reconnect it."
                                        .into();
                                report
                                    .details
                                    .push_str(&format!("\nConnection error: {error}"));
                            }
                        }
                    }
                    report.details.push_str(&format!(
                        "\nConnection ready: {}\nConnection: {}",
                        report.ready, report.summary
                    ));
                    was_ready = report.ready;
                    if tx.send(Event::Hardware(report)).is_err() {
                        break;
                    }
                }
                // Replugging or waking the machine is picked up automatically.
                for _ in 0..50 {
                    if shutdown.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
    }
}

pub fn attach_adapter() -> Result<(), String> {
    let listing = bounded_output(hidden_command("usbipd").arg("list"), Duration::from_secs(8))
        .map_err(|error| format!("Could not reach the USB service: {error}"))?;
    if !listing.status.success() {
        return Err("The AirBridge USB service could not list devices.".into());
    }
    let text = String::from_utf8_lossy(&listing.stdout);
    let row = hardware::supported_row(&text)
        .ok_or_else(|| "Plug in the Realtek AirBridge Wi-Fi adapter.".to_owned())?;
    if row.to_ascii_lowercase().contains("attached") {
        return Ok(());
    }
    let bus_id = row.split_whitespace().next().unwrap();
    let result = bounded_output(
        hidden_command("usbipd").args(["attach", "--wsl", "--busid", bus_id]),
        Duration::from_secs(20),
    )
    .map_err(|error| format!("Could not connect the Wi-Fi adapter: {error}"))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }
    Ok(())
}
