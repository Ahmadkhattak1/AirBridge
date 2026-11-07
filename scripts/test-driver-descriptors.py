#!/usr/bin/env python3
"""Exercise the patched driver's descriptor writers without radio hardware."""
import pathlib
import re
import subprocess
import sys
import tempfile


def function(source, name):
    start = re.search(r"static (?:void|int) " + re.escape(name) + r"\(", source).start()
    end = source.index("\n}\n", start) + 3
    return source[start:end]


def main():
    module = pathlib.Path(sys.argv[1])
    source = (module / "core.c").read_text()
    header = (module / "rtl8xxxu.h").read_text()
    defines = "\n".join(
        line for line in header.splitlines()
        if re.match(r"#define (TXDESC32_|TXDESC40_|TXDESC_DISABLE_DATA_FB\b|DESC_RATE_)", line)
        and "\\" not in line
    )
    descriptors = "\n".join(
        re.search(r"struct " + name + r" \{.*?\n};", header, re.S).group()
        for name in ("rtl8xxxu_txdesc32", "rtl8xxxu_txdesc40")
    )
    harness = r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
typedef uint8_t u8;
typedef uint16_t u16;
typedef uint16_t __le16;
typedef uint32_t __le32;
#define BIT(n) (1U << (n))
#define GENMASK(h,l) (((~0U) << (l)) & (~0U >> (31-(h))))
#define cpu_to_le32(n) ((uint32_t)(n))
#define ETH_ALEN 6
#define IEEE80211_TX_RC_MCS 1
#define IEEE80211_TX_RC_SHORT_GI 2
#define IEEE80211_TX_CTL_INJECTED 4
#define IEEE80211_TX_CTL_NO_ACK 8
#define NL80211_BAND_2GHZ 0
struct ieee80211_tx_rate { int idx; unsigned flags; };
struct ieee80211_tx_info {
    unsigned flags, band;
    struct { struct ieee80211_tx_rate rates[1]; } control;
};
struct ieee80211_hdr { u16 frame_control; u8 pad[14]; u8 addr3[6]; u16 seq_ctrl; };
static bool ieee80211_is_data(u16 fc) { return (fc & 0x0c) == 8; }
static bool ieee80211_is_action(u16 fc) { return (fc & 0xfc) == 0xd0; }
struct rtl8xxxu_fileops { int tx_desc_size; };
struct rtl8xxxu_priv { int tx_paths; struct rtl8xxxu_fileops *fops; };
static bool airbridge_inject_rate = true, airbridge_cck;
static bool airbridge_rx_amsdu = true;
'''
    cases = r'''
int main(void) {
    /* Full 7935-byte A-MSDU plus radio headers fits; existing USB batches
     * stay intact. The opt-out and ordinary driver behavior are preserved. */
    assert(rtl8xxxu_airbridge_rx_buffer_size(2352 + 24) >= 7935 + 26 + 4 + 24 + 32);
    assert(rtl8xxxu_airbridge_rx_buffer_size(32768) == 32768);
    airbridge_rx_amsdu = false;
    assert(rtl8xxxu_airbridge_rx_buffer_size(2376) == 2376);
    airbridge_rx_amsdu = true;
    airbridge_inject_rate = false;
    assert(rtl8xxxu_airbridge_rx_buffer_size(2376) == 2376);
    airbridge_inject_rate = true;
    struct rtl8xxxu_fileops fops = {0};
    struct rtl8xxxu_priv priv = {1, &fops};
    struct { struct ieee80211_hdr hdr; u8 body[8]; } frame = {0};
    struct ieee80211_tx_info info = {
        .flags = IEEE80211_TX_CTL_INJECTED,
        .control.rates = {{7, IEEE80211_TX_RC_MCS | IEEE80211_TX_RC_SHORT_GI}}
    };
    struct rtl8xxxu_txdesc40 desc, before;
    memcpy(frame.hdr.addr3, "\x00\x25\x00\xff\x94\x73", 6);
    frame.hdr.frame_control = 8;
    for (int size = 32; size <= 40; size += 8) {
        fops.tx_desc_size = size;
        memset(&desc, 0xff, sizeof(desc));
        rtl8xxxu_airbridge_txdesc(&priv, &frame.hdr, &info, (void *)&desc, 32);
        if (size == 40) {
            assert((desc.txdw4 & 0x7f) == DESC_RATE_MCS7);
            assert((desc.txdw3 & (BIT(8) | BIT(10))) == (BIT(8) | BIT(10)));
            assert(!(desc.txdw8 & TXDESC40_HW_SEQ_ENABLE));
            assert((desc.txdw4 & TXDESC40_RETRY_LIMIT_MASK) == (2U << 18));
            assert(desc.txdw5 & TXDESC40_SHORT_PREAMBLE);
        } else {
            assert((desc.txdw5 & 0x3f) == DESC_RATE_MCS7);
            assert((desc.txdw4 & (BIT(8) | BIT(10))) == (BIT(8) | BIT(10)));
            assert(!(desc.txdw4 & TXDESC32_HW_SEQ_ENABLE));
            assert((desc.txdw5 & TXDESC32_RETRY_LIMIT_MASK) == (2U << 18));
            assert(desc.txdw5 & TXDESC32_SHORT_GI);
            assert(desc.txdw8 == UINT32_MAX && desc.txdw9 == UINT32_MAX);
        }
        before = desc;
        info.flags = 0;
        rtl8xxxu_airbridge_txdesc(&priv, &frame.hdr, &info, (void *)&desc, 32);
        assert(!memcmp(&desc, &before, sizeof(desc))); /* Managed traffic unchanged. */
        info.flags = IEEE80211_TX_CTL_INJECTED | IEEE80211_TX_CTL_NO_ACK;
        rtl8xxxu_airbridge_txdesc(&priv, &frame.hdr, &info, (void *)&desc, 23);
        assert(!memcmp(&desc, &before, sizeof(desc))); /* Truncated header. */
        frame.hdr.addr3[0] = 1;
        rtl8xxxu_airbridge_txdesc(&priv, &frame.hdr, &info, (void *)&desc, 32);
        assert(!memcmp(&desc, &before, sizeof(desc))); /* Unrelated BSSID. */
        frame.hdr.addr3[0] = 0;
        info.control.rates[0].idx = 15;
        rtl8xxxu_airbridge_txdesc(&priv, &frame.hdr, &info, (void *)&desc, 32);
        assert(!memcmp(&desc, &before, sizeof(desc))); /* Reject unsupported stream. */
        priv.tx_paths = 2;
        rtl8xxxu_airbridge_txdesc(&priv, &frame.hdr, &info, (void *)&desc, 32);
        assert(((size == 40 ? desc.txdw4 : desc.txdw5) & 0x3f) == DESC_RATE_MCS15);
        assert(!((size == 40 ? desc.txdw4 : desc.txdw5) & (0x3fU << 18)));
        priv.tx_paths = 1;
        info.control.rates[0].idx = 11;
        info.control.rates[0].flags = 0;
        rtl8xxxu_airbridge_txdesc(&priv, &frame.hdr, &info, (void *)&desc, 32);
        assert(((size == 40 ? desc.txdw4 : desc.txdw5) & 0x3f) == DESC_RATE_54M);
        assert(!(desc.txdw5 &
                 (size == 40 ? TXDESC40_SHORT_PREAMBLE : TXDESC32_SHORT_GI)));
        info.control.rates[0].idx = 7;
        info.control.rates[0].flags = IEEE80211_TX_RC_MCS | IEEE80211_TX_RC_SHORT_GI;
        info.flags = IEEE80211_TX_CTL_INJECTED;
        airbridge_cck = true;
        frame.hdr.frame_control = 0xd0;
        memcpy(frame.body, "\x7f\x00\xe0\x25" "ABW1", 8);
        rtl8xxxu_airbridge_txdesc(&priv, &frame.hdr, &info, (void *)&desc, 32);
        assert(((size == 40 ? desc.txdw4 : desc.txdw5) & 0x3f) == DESC_RATE_11M);
        assert(!((size == 40 ? desc.txdw4 : desc.txdw5) & (0x3fU << 18)));
        airbridge_cck = false;
        frame.hdr.frame_control = 8;
    }
    return 0;
}
'''
    code = "\n".join((harness, defines, descriptors,
                       function(source, "rtl8xxxu_airbridge_rx_buffer_size"),
                       function(source, "rtl8xxxu_airbridge_set_rate"),
                       function(source, "rtl8xxxu_airbridge_txdesc"), cases))
    with tempfile.TemporaryDirectory(prefix="airbridge-descriptor-test-") as tmp:
        path = pathlib.Path(tmp)
        (path / "test.c").write_text(code)
        subprocess.run(["cc", "-Wall", "-Wextra", "-Werror", "-Wno-sign-compare",
                        "-fsanitize=address,undefined", "-o", str(path / "test"),
                        str(path / "test.c")], check=True)
        subprocess.run([str(path / "test")], check=True)
    print("AirBridge 32/40-byte descriptor regression checks passed")


if __name__ == "__main__":
    main()
