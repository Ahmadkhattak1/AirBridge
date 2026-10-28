use super::{AirBridgeApp, Event, bounded_output, hidden_command};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Report {
    pub ready: bool,
    pub summary: String,
    pub details: String,
}

impl AirBridgeApp {
    pub fn hardware_ready(&self) -> bool {
        self.preview || (!self.hardware_checking && self.hardware.as_ref().is_some_and(|r| r.ready))
    }

    pub fn check_hardware(&mut self) {
        if self.hardware_checking || self.listening || self.sending {
            return;
        }
        self.hardware_checking = true;
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Hardware(probe()));
        });
    }
}

// Only connected rows count; usbipd retains entries for unplugged devices.
fn supported_row(listing: &str) -> Option<&str> {
    listing
        .lines()
        .take_while(|line| !line.trim().eq_ignore_ascii_case("Persisted:"))
        .find(|line| {
            line.split_whitespace()
                .nth(1)
                .is_some_and(|id| id.eq_ignore_ascii_case("0bda:f179"))
        })
}

pub fn probe() -> Report {
    let usb = bounded_output(hidden_command("usbipd").arg("list"), Duration::from_secs(8));
    let usb_ok = usb.as_ref().is_ok_and(|o| o.status.success());
    let listing = usb
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let row = supported_row(&listing);
    let shared = row.is_some_and(|r| !r.to_ascii_lowercase().contains("not shared"));
    // Collect only device model IDs, not instance IDs/serial numbers or network identifiers.
    let script = r#"$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); $present=@(Get-PnpDevice -PresentOnly | Select-Object -ExpandProperty InstanceId); $devices=@(Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.DeviceID -in $present -and ($_.DeviceClass -eq 'NET' -or $_.DeviceID -like 'USB\VID_0BDA&PID_F179*') } | ForEach-Object { [pscustomobject]@{Name=$_.DeviceName; HardwareId=($_.DeviceID -split '\\')[1]; Driver=$_.DriverVersion; Provider=$_.DriverProviderName} }); [pscustomobject]@{Windows=[Environment]::OSVersion.Version.ToString(); Devices=$devices} | ConvertTo-Json -Depth 4 -Compress"#;
    let inventory = bounded_output(
        hidden_command("powershell.exe").args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ]),
        Duration::from_secs(15),
    );
    let inventory = match inventory {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        _ => "Device inventory unavailable. Try checking again.".into(),
    };
    let wsl = bounded_output(
        hidden_command("wsl.exe").args(["--list", "--quiet"]),
        Duration::from_secs(8),
    );
    let runtime = wsl.is_ok_and(|o| {
        let units: Vec<u16> = o
            .stdout
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        o.status.success()
            && (String::from_utf16_lossy(&units)
                .lines()
                .any(|s| s.trim() == "AirBridgeRuntime")
                || String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|s| s.trim() == "AirBridgeRuntime"))
    });
    let summary = if !usb_ok {
        "USB service setup required"
    } else if row.is_none() {
        "No supported Wi-Fi adapter connected"
    } else if !runtime {
        "Supported adapter · AirBridge runtime setup required"
    } else if !shared {
        "Supported adapter · USB sharing setup required"
    } else {
        "Supported adapter · Realtek RTL8188FTV"
    };
    Report {
        ready: usb_ok && row.is_some() && runtime && shared,
        summary: summary.into(),
        details: format!(
            "AirBridge {} compatibility report\nStatus: {summary}\nVerified USB ID: 0bda:f179 (RTL8188FTV)\nUSB service available: {usb_ok}\nMatching adapter connected: {}\nUSB shared: {shared}\nRuntime registered: {runtime}\n{inventory}\n\nPlease add the adapter's brand/model and hardware revision from its label when reporting this on GitHub. No files, IP addresses, MAC addresses or serial numbers are collected. Driver versions alone do not establish AirDrop compatibility.",
            env!("CARGO_PKG_VERSION"),
            row.is_some()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unplugged_or_other_chipsets_are_not_supported() {
        assert!(
            supported_row(
                "Connected:\n1-1 0bda:8179 Other Not shared\nPersisted:\nfoo 0bda:f179 Realtek"
            )
            .is_none()
        );
        assert!(supported_row("Connected:\n1-2 0BDA:F179 Realtek Shared\nPersisted:").is_some());
    }
}
