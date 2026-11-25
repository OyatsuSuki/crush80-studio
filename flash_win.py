#!/usr/bin/env python3
"""Windows HIDAPI adapter for wobkey-crush80-reboot-pro-firmware/tools/crush80.py.

Reuses 100% of the upstream safety guards, SHA-256 allowlist, source CRC checks,
packet framing, and post-flash verification, using the PyPI `hidapi` (`hid`) package.
"""
import sys
from pathlib import Path
import hid

REPO_TOOLS = Path(__file__).resolve().parent / "wobkey-crush80-reboot-pro-firmware" / "tools"
sys.path.insert(0, str(REPO_TOOLS))
import crush80  # noqa: E402


class PyHidAdapter:
    def __init__(self):
        self.dev = None

    def enumerate(self):
        result = []
        for d in hid.enumerate(crush80.VID, 0):
            result.append(
                dict(
                    path=d["path"],
                    pid=d["product_id"],
                    usage=d["usage_page"],
                    interface=d["interface_number"],
                    product=d["product_string"],
                    release=d["release_number"],
                )
            )
        return result

    def open(self):
        devices = [d for d in self.enumerate() if d["pid"] == crush80.PID and d["usage"] == crush80.USAGE]
        if len(devices) != 1:
            raise RuntimeError(
                f"Expected exactly one wired Crush 80 OTA interface; found {len(devices)}. "
                "Connect by USB cable and select wired mode."
            )
        dev = hid.device()
        dev.open_path(devices[0]["path"])
        self.dev = dev
        return devices[0]

    def write(self, data: bytes):
        n = self.dev.write(data)
        if n != len(data):
            raise OSError(f"HID write returned {n}/{len(data)}")

    def read(self, timeout_ms: int) -> bytes:
        raw = self.dev.read(crush80.PACKET_SIZE, timeout_ms=timeout_ms)
        return bytes(raw) if raw else b""

    def close(self):
        if self.dev is not None:
            try:
                self.dev.close()
            finally:
                self.dev = None

    def shutdown(self):
        self.close()


if __name__ == "__main__":
    # Register our rc2emu-verified custom patches (WebRainbow & ProSuite)
    WEBRAINBOW_SHA = "c7c243d47c356c47503397bf05427ac542d797537d53854167b8b1106c6a5bf2"
    WEBRAINBOW_CRC = 0xAF713C2C
    PROSUITE_SHA = "8bc0779343ff2d82c611a48303391e494679f05fce739db41868c54bed1418fd"
    PROSUITE_CRC = 0x99FCFDBD
    PROSUITE2_SHA = "91397d17bfbcff7829c368518e048dfd833b6473b5378efa444109228d8513e5"
    PROSUITE2_CRC = 0x4859A8C5
    PROSUITE3_SHA = "4a504a0337dec774938b6392f1480714b099609b43e006e827804b2b73461599"
    PROSUITE3_CRC = 0xC03D5821
    EMBER12_OLD_CRC = 0x6C8D7399
    EMBER12_SHA = "56125ed4ec7a6edf5e05780f8fc86055863ecc39d0bf3ead2dc21bfce8a3e11c"
    EMBER12_CRC = 0x0AACEB06
    EMBER13_SHA = "58b8689c1d4af5e27dc32cf4646f2d1921507d72f5d61cf0a9a9b611ebbb2aae"
    EMBER13_CRC = 0x676B6326

    ALL_CUSTOM = (WEBRAINBOW_CRC, PROSUITE_CRC, PROSUITE2_CRC, PROSUITE3_CRC, EMBER12_OLD_CRC, EMBER12_CRC, EMBER13_CRC)
    crush80.APPROVED_FLASH_IMAGES[WEBRAINBOW_SHA] = {
        "name": "NativeHeld v1.3.0 + WebRainbow (VIA id=5)",
        "crc": WEBRAINBOW_CRC,
        "from_crcs": (0xCA2D85DC, 0x2F4F4974) + ALL_CUSTOM,
    }
    crush80.APPROVED_FLASH_IMAGES[PROSUITE_SHA] = {
        "name": "NativeHeld v1.3.0 + ProSuite v1",
        "crc": PROSUITE_CRC,
        "from_crcs": (0xCA2D85DC, 0x2F4F4974) + ALL_CUSTOM,
    }
    crush80.APPROVED_FLASH_IMAGES[PROSUITE2_SHA] = {
        "name": "NativeHeld v1.3.0 + ProSuite v2",
        "crc": PROSUITE2_CRC,
        "from_crcs": (0xCA2D85DC, 0x2F4F4974) + ALL_CUSTOM,
    }
    crush80.APPROVED_FLASH_IMAGES[PROSUITE3_SHA] = {
        "name": "Ember (True Battery)",
        "crc": PROSUITE3_CRC,
        "from_crcs": (0xCA2D85DC, 0x2F4F4974) + ALL_CUSTOM,
    }
    crush80.APPROVED_FLASH_IMAGES[EMBER12_SHA] = {
        "name": "Ember (Logo LED Feedback)",
        "crc": EMBER12_CRC,
        "from_crcs": (0xCA2D85DC, 0x2F4F4974) + ALL_CUSTOM,
    }
    crush80.APPROVED_FLASH_IMAGES[EMBER13_SHA] = {
        "name": "Ember v1.3.0 (0xFD GET/SET Guard)",
        "crc": EMBER13_CRC,
        "from_crcs": (0xCA2D85DC, 0x2F4F4974) + ALL_CUSTOM,
    }
    # Allow seamless rollback from custom builds back to Official 0108 or stock NativeHeld v1.3.0
    for pol in crush80.APPROVED_FLASH_IMAGES.values():
        if pol["from_crcs"] is not None:
            for c in ALL_CUSTOM:
                if c not in pol["from_crcs"]:
                    pol["from_crcs"] = tuple(pol["from_crcs"]) + (c,)
    crush80.HID = PyHidAdapter
    try:
        crush80.main()
    except (OSError, RuntimeError, TimeoutError, ValueError) as exc:
        print("ERROR: " + str(exc), file=sys.stderr)
        sys.exit(1)
