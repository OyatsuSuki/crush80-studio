#!/usr/bin/env python3
"""Read-only VIA backup (and optional restore) tool for WOBKEY Crush 80 Reboot Pro / Lite.

Backs up:
  - All 4 layers of keymap (1024 bytes total: 4 layers x 8 rows x 16 cols x 2 bytes)
  - Full 512-byte macro buffer
  - Current RGB settings (brightness, effect, speed, hue, saturation, rainbow)
  - Optional Ember hardware config registers (CONFIG[21..60]) when Ember firmware is detected
"""
import argparse
import json
import time
from pathlib import Path
import hid

VID = 0x320F
PID_USB = 0x5055
VIA_USAGE_PAGE = 0xFF60
KEYMAP_BYTES = 1024  # 4 layers * 256 bytes/layer
MACRO_BYTES = 512
CHUNK_SIZE = 16


class ViaClient:
    def __init__(self):
        devs = [d for d in hid.enumerate(VID, 0) if d["usage_page"] == VIA_USAGE_PAGE]
        if not devs:
            raise RuntimeError("No VIA HID interface (0xFF60) found. Connect via USB wired or 2.4G mode.")
        # Prefer wired USB (PID 0x5055) if both wired USB and 2.4G dongle are present
        usb_devs = [d for d in devs if d.get("product_id") == PID_USB]
        self.info = usb_devs[0] if usb_devs else devs[0]
        self.dev = hid.device()
        self.dev.open_path(self.info["path"])

    def close(self):
        self.dev.close()

    def cmd(self, payload: bytes) -> bytes:
        if not payload or len(payload) > 32:
            raise ValueError(f"Invalid VIA payload length: {len(payload)}")
        pkt = bytes([0x00]) + payload + bytes(32 - len(payload))
        written = self.dev.write(pkt)
        if written <= 0:
            raise OSError(f"HID write failed for VIA command 0x{payload[0]:02X} (ret={written})")
        resp = bytes(self.dev.read(32, timeout_ms=2000))
        if not resp:
            raise TimeoutError(f"VIA command 0x{payload[0]:02X} timed out")
        if len(resp) < 32:
            raise RuntimeError(
                f"Short VIA response for 0x{payload[0]:02X}: expected 32 bytes, got {len(resp)}"
            )
        if resp[0] != payload[0]:
            raise RuntimeError(
                f"VIA response opcode mismatch: expected 0x{payload[0]:02X}, got 0x{resp[0]:02X}"
            )
        return resp

    def backup(self) -> dict:
        # 1. Keymap buffer (cmd 0x12: id_dynamic_keymap_get_buffer)
        keymap = bytearray()
        for offset in range(0, KEYMAP_BYTES, CHUNK_SIZE):
            size = min(CHUNK_SIZE, KEYMAP_BYTES - offset)
            req = bytes([0x12, (offset >> 8) & 0xFF, offset & 0xFF, size])
            resp = self.cmd(req)
            keymap.extend(resp[4 : 4 + size])

        # 2. Macro buffer (cmd 0x0E: id_dynamic_keymap_macro_get_buffer)
        macros = bytearray()
        for offset in range(0, MACRO_BYTES, CHUNK_SIZE):
            size = min(CHUNK_SIZE, MACRO_BYTES - offset)
            req = bytes([0x0E, (offset >> 8) & 0xFF, offset & 0xFF, size])
            resp = self.cmd(req)
            macros.extend(resp[4 : 4 + size])

        # 3. RGB settings (cmd 0x08, channel 0x03)
        rgb = {}
        for val_id, name in [(1, "brightness"), (2, "effect"), (3, "speed")]:
            resp = self.cmd(bytes([0x08, 0x03, val_id]))
            rgb[name] = resp[3]
        resp_color = self.cmd(bytes([0x08, 0x03, 4]))
        rgb["hue"] = resp_color[3]
        rgb["saturation"] = resp_color[4]
        resp_rb = self.cmd(bytes([0x08, 0x03, 5, 0xAA]))
        rgb["rainbow"] = resp_rb[3] == 0xFF

        # 4. Optional Ember hardware config registers (probed via sentinel 0xAA on CONFIG[26])
        hw = None
        os_probe = self.cmd(bytes([0x08, 0x03, 26, 0xAA]))[3]
        if os_probe != 0xAA:
            get_reg = lambda rid: self.cmd(bytes([0x08, 0x03, rid]))[3]
            tt_units = get_reg(60)
            logo_rb = get_reg(33) != 0
            side_rb = get_reg(37) != 0
            hw = {
                "win_lock": get_reg(21) != 0,
                "mac_mode": os_probe != 0,
                "low_latency": get_reg(29) != 0,
                "logo_mode": get_reg(32),
                "logo_color": 0xFF if logo_rb else min(get_reg(34), 44),
                "logo_on": get_reg(35) == 0,
                "side_mode": get_reg(36),
                "side_color": 0xFF if side_rb else min(get_reg(38), 44),
                "side_on": get_reg(39) == 0,
                "sys_rgb_on": get_reg(40) == 0,
                "tapping_term_ms": 200 if tt_units == 0 else tt_units * 10,
                "prosuite_supported": True,
            }

        # Decode keycodes per layer for human readability (big-endian uint16)
        layers = []
        for layer in range(4):
            base = layer * 256
            codes = [
                f"0x{(keymap[base + i] << 8) | keymap[base + i + 1]:04X}"
                for i in range(0, 256, 2)
            ]
            layers.append(codes)

        out = {
            "timestamp": time.strftime("%Y-%m-%d %H:%M:%S"),
            "device": {
                "vid": f"0x{VID:04X}",
                "pid": f"0x{self.info['product_id']:04X}",
                "release": f"0x{self.info['release_number']:04X}",
                "product": self.info["product_string"],
            },
            "rgb": rgb,
            "keymap_hex": keymap.hex(),
            "macros_hex": macros.hex(),
            "layers_preview": layers,
        }
        if hw is not None:
            out["hw"] = hw
        return out

    def restore(self, data: dict):
        keymap = bytes.fromhex(data["keymap_hex"])
        macros = bytes.fromhex(data["macros_hex"])
        if len(keymap) != KEYMAP_BYTES:
            raise ValueError(f"Invalid keymap length: expected {KEYMAP_BYTES}, got {len(keymap)}")
        if len(macros) != MACRO_BYTES:
            raise ValueError(f"Invalid macro length: expected {MACRO_BYTES}, got {len(macros)}")

        print("Restoring keymap (4 layers)...")
        for offset in range(0, KEYMAP_BYTES, CHUNK_SIZE):
            chunk = keymap[offset : offset + CHUNK_SIZE]
            req = bytes([0x13, (offset >> 8) & 0xFF, offset & 0xFF, len(chunk)]) + chunk
            self.cmd(req)

        print("Restoring macros (512 bytes)...")
        for offset in range(0, MACRO_BYTES, CHUNK_SIZE):
            chunk = macros[offset : offset + CHUNK_SIZE]
            req = bytes([0x0F, (offset >> 8) & 0xFF, offset & 0xFF, len(chunk)]) + chunk
            self.cmd(req)

        rgb = data.get("rgb", {})
        if rgb:
            print("Restoring RGB settings...")
            self.cmd(bytes([0x07, 0x03, 1, min(int(rgb["brightness"]), 9)]))
            self.cmd(bytes([0x07, 0x03, 2, min(int(rgb["effect"]), 18)]))
            self.cmd(bytes([0x07, 0x03, 3, min(int(rgb["speed"]), 4)]))
            if rgb.get("rainbow"):
                self.cmd(bytes([0x07, 0x03, 5, 0xFF]))
            else:
                self.cmd(bytes([0x07, 0x03, 4, int(rgb["hue"]) & 0xFF, int(rgb["saturation"]) & 0xFF]))

        hw = data.get("hw")
        if isinstance(hw, dict) and hw.get("prosuite_supported"):
            print("Restoring Ember hardware config...")
            self.cmd(bytes([0x07, 0x03, 21, 0xFF if hw.get("win_lock") else 0x00]))
            self.cmd(bytes([0x07, 0x03, 26, 0x01 if hw.get("mac_mode") else 0x00]))
            self.cmd(bytes([0x07, 0x03, 29, 0xFF if hw.get("low_latency") else 0x00]))
            self.cmd(bytes([0x07, 0x03, 32, min(int(hw.get("logo_mode", 0)), 3)]))
            lc = int(hw.get("logo_color", 0xFF))
            if lc == 0xFF:
                self.cmd(bytes([0x07, 0x03, 33, 0xFF]))
            else:
                self.cmd(bytes([0x07, 0x03, 33, 0x00]))
                self.cmd(bytes([0x07, 0x03, 34, min(lc, 44)]))
            self.cmd(bytes([0x07, 0x03, 35, 0x00 if hw.get("logo_on", True) else 0x01]))
            self.cmd(bytes([0x07, 0x03, 36, min(int(hw.get("side_mode", 0)), 3)]))
            sc = int(hw.get("side_color", 0xFF))
            if sc == 0xFF:
                self.cmd(bytes([0x07, 0x03, 37, 0xFF]))
            else:
                self.cmd(bytes([0x07, 0x03, 37, 0x00]))
                self.cmd(bytes([0x07, 0x03, 38, min(sc, 44)]))
            self.cmd(bytes([0x07, 0x03, 39, 0x00 if hw.get("side_on", True) else 0x01]))
            self.cmd(bytes([0x07, 0x03, 40, 0x00 if hw.get("sys_rgb_on", True) else 0x01]))
            tt_ms = int(hw.get("tapping_term_ms", 200))
            tt_units = 0 if tt_ms == 0 else max(1, min(255, tt_ms // 10))
            self.cmd(bytes([0x07, 0x03, 60, tt_units]))

        if rgb or hw:
            # Save lighting & config (cmd 0x09)
            self.cmd(bytes([0x09]))
        print("Restore complete!")


def main():
    parser = argparse.ArgumentParser(description="Crush 80 VIA Backup/Restore Tool")
    parser.add_argument("action", choices=["backup", "restore"])
    parser.add_argument("file", type=Path)
    args = parser.parse_args()

    client = ViaClient()
    try:
        if args.action == "backup":
            data = client.backup()
            args.file.write_text(json.dumps(data, indent=2, ensure_ascii=False), encoding="utf-8")
            non_zero_macros = sum(1 for b in bytes.fromhex(data["macros_hex"]) if b != 0)
            print(f"Backup saved to {args.file}")
            print(f"  RGB: {data['rgb']}")
            print(f"  Keymap: {KEYMAP_BYTES} bytes (4 layers)")
            print(f"  Macros: {MACRO_BYTES} bytes ({non_zero_macros} non-zero bytes)")
        else:
            data = json.loads(args.file.read_text(encoding="utf-8"))
            client.restore(data)
    finally:
        client.close()


if __name__ == "__main__":
    main()
