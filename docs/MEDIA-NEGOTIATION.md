# Preserving HEVC HDR uploads

AirBridge saves the offered bitstream; it does not transcode or play video.
The receiver media profile requests preservation, not a guarantee of decoding
support in the user's installed Windows player.

## Native conversion check, 2026-11-07

The original test video was 165,300,237 bytes, HEVC Main 10 level 153,
3840 × 2160, about 59.94 fps, HLG/BT.2020. Earlier iPhone uploads to Windows
were 40,482,505-byte AVC/BT.709 videos at 30 fps. These are different exports;
their transfer times alone are not an equal-byte throughput comparison.

On macOS 15.7.3 we evaluated the original with the conversion preflight used
by sharingd: `PHMediaFormatConversionSource.sourceForFileURL:`,
`PHMediaFormatConversionRequest.requestForSource:destinationCapabilities:error:`,
`setUseTransferBehaviorUserPreference:YES`, and
`preflightWithConversionManager:`. No encoding was needed for this check.

| Profile change | Requires conversion | Exports HDR |
| --- | --- | --- |
| Previous AirBridge profile, including HDR allowed | true | false |
| Add only maximum decode level | true | false |
| Add only Dolby Vision fields | true | false |
| Add only HEVC profile 4 | true | false |
| Set per-profile acceleration hint false | true | false |
| Set per-profile acceleration hint true for Main/Main 10 | false | true |
| Native Mac profile with acceleration hints removed | true | false |
| Exact updated AirBridge JSON | false | true |

The missing key is `VTIsHardwareAccelerated` in `VTPerProfileSupport`.
Apple uses it as a preservation eligibility hint. AirBridge uses the hint
to request originals; this is not a Windows GPU capability report. Copying
Mac model identifiers, identity records or the entire Mac profile was unnecessary.
The earlier `PFMediaCapabilities.decodingSupportForAVAsset:` query accepted
HDR alone but did not test whether Photos would still choose conversion.

The wire regression parses the Discover reply and checks both hints. Native
preflight is additional evidence, not an iPhone end-to-end test. The next
iPhone test must verify saved codec, color metadata, frame count, byte size
(and source hash when available), plus sender success.

## HTTP/3 status

The stream tests validate server behavior, consent and archive integrity.
They do not prove native discovery. The Mac uses ApplicationService/Rapport
discovery; a bare `_asquic._udp` record does not provide that association.
All measured Windows transfers used TCP. HTTP/3 is opt-in experimental
(`AIRBRIDGE_HTTP3=1`) until native discovery is implemented and verified.

## Radio corrections

The selected band's `iw phy` report now supplies HT capabilities, A-MPDU
spacing/length, and the receive MCS mask. The old constant advertised
`0x11ce/0x1b`; the measured RTL8188FTV reports `0x006c/0x1f`, including
short GI at 20 MHz. Both `HT RX` and `HT TX/RX` MCS formats are parsed.
This is a corrected advertisement, not proof the sender uses short GI.

Observed-presence TCP ACKs may now pass queued neighbor probes and other
connections while retaining order within their own TCP flow. The one-use,
one-TU, same-slot limit is unchanged. A deterministic regression exercises
the previously blocked ACK and verifies remaining packets still drain.
Neither fix has a measured iPhone throughput gain yet.
