#!/usr/bin/env python3
"""Generate, but do not apply, the opt-in RTL8188F experiment patch (GPL-2.0)."""
import difflib
from pathlib import Path

source = Path('/opt/airbridge/kernel/drivers/net/wireless/realtek/rtl8xxxu/core.c')
before = source.read_text()
assert 'airbridge_cck' not in before, 'patch is already applied; do not layer changes'
after = before.replace('static bool rtl8xxxu_ht40_2g;', '''static bool rtl8xxxu_ht40_2g;
/* Isolated CCK experiment; disabled for all normal AirBridge traffic. */
static bool airbridge_cck;
module_param(airbridge_cck, bool, 0600);
MODULE_PARM_DESC(airbridge_cck, "Enable marked AirBridge 11Mbps CCK test frames");''', 1)
old = '\trtl8xxxu_calc_tx_desc_csum(tx_desc);'
new = '''	/* Only our explicit vendor-action probe is changed. Preserve normal AWDL.
	 * The marker is outside the synthesized payload, at MAC-body offset 24.
	 * RTL8188F uses the 40-byte gen2 descriptor; rate 3 is 11Mbps CCK.
	 */
	if (airbridge_cck && priv->rtl_chip == RTL8188F && pktlen >= 32 &&
	    (tx_info->flags & IEEE80211_TX_CTL_INJECTED) &&
	    ieee80211_is_action(hdr->frame_control) &&
	    !memcmp((u8 *)hdr + 24, "\\x7f\\x00\\xe0\\x25" "ABW1", 8)) {
		struct rtl8xxxu_txdesc40 *test_desc = (void *)tx_desc;

		test_desc->txdw3 |= cpu_to_le32(TXDESC40_USE_DRIVER_RATE | BIT(10));
		test_desc->txdw4 &= cpu_to_le32(~(0x7fU | TXDESC40_DATA_RATE_FB_MASK |
				TXDESC40_RETRY_LIMIT_MASK));
		test_desc->txdw4 |= cpu_to_le32(DESC_RATE_11M | TXDESC40_RETRY_LIMIT_ENABLE);
		test_desc->txdw8 &= cpu_to_le32(~TXDESC40_HW_SEQ_ENABLE);
		dev_info_ratelimited(dev, "AirBridge CCK probe: bytes=%u rate=%u seq=%u\\n",
			pktlen, le32_to_cpu(test_desc->txdw4) & 0x7f,
			le16_to_cpu(hdr->seq_ctrl));
	}

''' + old
# There is another checksum call elsewhere. Target only the one after fill_txdesc.
anchor = 'ampdu_enable, rts_rate, macid);\n\n' + old
assert before.count(anchor) == 1
after = after.replace(anchor, 'ampdu_enable, rts_rate, macid);\n\n' + new)
assert after != before
relative = 'drivers/net/wireless/realtek/rtl8xxxu/core.c'
patch = ''.join(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
                                   fromfile='a/' + relative, tofile='b/' + relative))
destination = Path(__file__).with_name('rtl8188f-cck-experiment.patch')
destination.write_text(patch)
print(patch)
