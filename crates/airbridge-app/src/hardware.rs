use super::{AirBridgeApp, bounded_output, hidden_command};
use std::time::Duration;

const ADAPTERS: &str = include_str!("../../../runtime/adapters.tsv");
const NO_ADAPTER: &str = "No supported Wi-Fi adapter connected";
const MULTIPLE_ADAPTERS: &str =
    "Multiple supported Wi-Fi adapters connected. Unplug the extra adapters.";

#[derive(Clone, Debug)]
pub struct Report {
    pub ready: bool,
    pub summary: String,
    pub details: String,
}

#[derive(Debug)]
pub struct Adapter {
    pub usb_id: &'static str,
    pub chipset: &'static str,
    pub driver: &'static str,
    pub firmware: &'static str,
    pub tx_rate: &'static str,
}

impl AirBridgeApp {
    pub fn hardware_ready(&self) -> bool {
        self.preview || (!self.hardware_checking && self.hardware.as_ref().is_some_and(|r| r.ready))
    }
}

fn adapters() -> impl Iterator<Item = Adapter> {
    ADAPTERS.lines().filter_map(|line| {
        if line.starts_with('#') || line.is_empty() {
            return None;
        }
        let fields: Vec<_> = line.split('\t').collect();
        match fields.as_slice() {
            [usb_id, chipset, driver, firmware, tx_rate] => Some(Adapter {
                usb_id,
                chipset,
                driver,
                firmware,
                tx_rate,
            }),
            _ => None,
        }
    })
}

pub fn adapter_for_id(id: &str) -> Option<Adapter> {
    adapters().find(|adapter| adapter.usb_id.eq_ignore_ascii_case(id))
}

// Only connected rows count; usbipd retains entries for unplugged devices.
// Preserve an already attached adapter when another supported one is plugged in.
pub fn supported_row(listing: &str) -> Result<&str, &'static str> {
    let rows: Vec<_> = listing
        .lines()
        .take_while(|line| !line.trim().eq_ignore_ascii_case("Persisted:"))
        .filter(|line| {
            let mut fields = line.split_whitespace();
            let bus = fields.next().unwrap_or_default();
            let id = fields.next().unwrap_or_default();
            bus.contains('-')
                && bus.bytes().all(|b| b.is_ascii_digit() || b == b'-')
                && adapter_for_id(id).is_some()
        })
        .collect();
    let attached: Vec<_> = rows
        .iter()
        .copied()
        .filter(|row| row.split_whitespace().last() == Some("Attached"))
        .collect();
    match (attached.as_slice(), rows.as_slice()) {
        ([row], _) | ([], [row]) => Ok(*row),
        ([], []) => Err(NO_ADAPTER),
        _ => Err(MULTIPLE_ADAPTERS),
    }
}

pub fn probe() -> Report {
    let usb = bounded_output(hidden_command("usbipd").arg("list"), Duration::from_secs(8));
    let usb_ok = usb.as_ref().is_ok_and(|o| o.status.success());
    let listing = usb
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let selection = supported_row(&listing);
    let row = selection.as_ref().ok().copied();
    let shared = row.is_some_and(|r| !r.to_ascii_lowercase().contains("not shared"));
    let adapter = row
        .and_then(|r| r.split_whitespace().nth(1))
        .and_then(adapter_for_id);
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
    } else if let Err(message) = selection {
        message
    } else if !runtime {
        "Supported adapter · AirBridge runtime setup required"
    } else if !shared {
        "Supported adapter · USB sharing setup required"
    } else {
        "Adapter setup available"
    };
    let adapter_details = adapter.map_or_else(
        || "Selected adapter: none".into(),
        |a| {
            format!(
                "Selected USB ID: {}\nChipset: {}\nDriver: {}\nFirmware: {}\nTransmit rate: {}",
                a.usb_id, a.chipset, a.driver, a.firmware, a.tx_rate
            )
        },
    );
    Report {
        ready: usb_ok && row.is_some() && runtime && shared,
        summary: summary.into(),
        details: format!(
            "AirBridge {} compatibility report\nStatus: {summary}\n{adapter_details}\nUSB service available: {usb_ok}\nMatching adapter connected: {}\nUSB shared: {shared}\nRuntime registered: {runtime}",
            env!("CARGO_PKG_VERSION"),
            row.is_some()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_has_unique_ids_and_consistent_chipset_profiles() {
        let mut ids = HashSet::new();
        let mut profiles = std::collections::HashMap::new();
        for adapter in adapters() {
            assert!(ids.insert(adapter.usb_id));
            assert_eq!(adapter.usb_id.len(), 9);
            assert_eq!(adapter.usb_id.as_bytes()[4], b':');
            assert!(
                adapter
                    .usb_id
                    .bytes()
                    .enumerate()
                    .all(|(i, b)| i == 4 || b.is_ascii_hexdigit())
            );
            assert_eq!(adapter.driver, "rtl8xxxu");
            assert!(adapter.firmware.starts_with("rtlwifi/"));
            assert!(!adapter.firmware.contains(".."));
            assert_eq!(adapter.tx_rate, "mcs7");
            let profile = (adapter.driver, adapter.firmware, adapter.tx_rate);
            if let Some(previous) = profiles.insert(adapter.chipset, profile) {
                assert_eq!(profile, previous);
            }
        }
        assert!(profiles.len() >= 4);
        assert_eq!(
            ids.len(),
            ADAPTERS
                .lines()
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .count()
        );
    }

    #[test]
    fn unplugged_and_unknown_devices_are_not_selected() {
        assert!(
            supported_row(
                "Connected:\n1-1 1234:5678 Other Not shared\nPersisted:\nfoo 0bda:f179 Realtek"
            )
            .is_err()
        );
        assert!(supported_row("Connected:\n1-2 0BDA:F179 Realtek Shared\nPersisted:").is_ok());
    }

    #[test]
    fn supported_chipsets_and_oem_ids_can_replace_the_original_adapter() {
        for (id, chipset) in [
            ("0bda:f179", "RTL8188F"),
            ("0bda:8179", "RTL8188E"),
            ("2357:010c", "RTL8188E"),
            ("0bda:818b", "RTL8192E"),
            ("0b05:18f1", "RTL8192F"),
        ] {
            let listing = format!(
                "Connected:\n1-2 {id} USB Wi-Fi Shared\nPersisted:\nold-id 0bda:f179 Realtek"
            );
            let row = supported_row(&listing).unwrap();
            let selected = adapter_for_id(row.split_whitespace().nth(1).unwrap()).unwrap();
            assert_eq!(selected.chipset, chipset);
        }
    }

    #[test]
    fn ambiguous_adapters_require_selection_but_one_attached_adapter_is_reused() {
        let multiple = "Connected:\n1-2 0bda:f179 Realtek Shared\n1-3 0bda:8179 Realtek Shared";
        assert_eq!(supported_row(multiple), Err(MULTIPLE_ADAPTERS));
        let attached = multiple.replacen("Shared", "Attached", 1);
        assert!(supported_row(&attached).unwrap().starts_with("1-2 "));
        let both_attached = multiple.replace("Shared", "Attached");
        assert_eq!(supported_row(&both_attached), Err(MULTIPLE_ADAPTERS));
        let unplugged =
            "Connected:\n1-3 0bda:8179 Realtek Shared\nPersisted:\n1-2 0bda:f179 Realtek";
        assert!(supported_row(unplugged).unwrap().starts_with("1-3 "));
    }
}
