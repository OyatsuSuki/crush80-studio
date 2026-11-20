#!/usr/bin/env python3
"""Read-only VIA backup (and optional restore) tool for WOBKEY Crush 80 Reboot Pro.

Backs up:
  - All 4 layers of keymap (1024 bytes total: 4 layers x 8 rows x 16 cols x 2 bytes)
  - Full 512-byte macro buffer
  - Current RGB settings (brightness, effect, speed, hue, saturation)
"""
import argparse
import json
import time
from pathlib import Path
import hid

VID = 0x320F
VIA_USAGE_PAGE = 0xFF60
KEYMAP_BYTES = 1024  # 4 layers * 256 bytes/layer
MACRO_BYTES = 512
CHUNK_SIZE = 16


class ViaClient:
    def __init__(self):
        devs = [d for d in hid.enumerate(VID, 0) if d["usage_page"] == VIA_USAGE_PAGE]
        if not devs:
            raise RuntimeError("No VIA HID interface (0xFF60) found. Connect via USB wired mode.")
        self.info = devs[0]
        self.dev = hid.device()
        self.dev.open_path(self.info["path"])

    def close(self):
        self.dev.close()

    def cmd(self, payload: bytes) -> bytes:
        pkt = bytes([0x00]) + payload + bytes(32 - len(payload))
        self.dev.write(pkt)
        resp = bytes(self.dev.read(32, timeout_ms=2000))
        if not resp:
            raise TimeoutError(f"VIA command 0x{payload[0]:02X} timed out")
        return resp

    def backup(self) -> dict:
        # 1. Keymap buffer (cmd 0x12: id_dynamic_keymap_get_buffer)
        keymap = bytearray()
        for offset in range(0, KEYMAP_BYTES, CHUNK_SIZE):
            size = min(CHUNK_SIZE, KEYMAP_BYTES - offset)
            req = bytes([0x12, (offset >> 8) & 0xFF, offset & 0xFF, size])
            resp = self.cmd(req)
            if resp[0] != 0x12:
                raise RuntimeError(f"Unexpected keymap read response at offset {offset}: {resp[:8].hex()}")
            keymap.extend(resp[4 : 4 + size])

        # 2. Macro buffer (cmd 0x0E: id_dynamic_keymap_macro_get_buffer)
        macros = bytearray()
        for offset in range(0, MACRO_BYTES, CHUNK_SIZE):
            size = min(CHUNK_SIZE, MACRO_BYTES - offset)
            req = bytes([0x0E, (offset >> 8) & 0xFF, offset & 0xFF, size])
            resp = self.cmd(req)
            if resp[0] != 0x0E:
                raise RuntimeError(f"Unexpected macro read response at offset {offset}: {resp[:8].hex()}")
            macros.extend(resp[4 : 4 + size])

        # 3. RGB settings (cmd 0x08, channel 0x03)
        rgb = {}
        for val_id, name in [(1, "brightness"), (2, "effect"), (3, "speed")]:
            resp = self.cmd(bytes([0x08, 0x03, val_id]))
            rgb[name] = resp[3]
        resp_color = self.cmd(bytes([0x08, 0x03, 4]))
        rgb["hue"] = resp_color[3]
        rgb["saturation"] = resp_color[4]

        # Decode keycodes per layer for human readability (big-endian uint16)
        layers = []
        for layer in range(4):
            base = layer * 256
            codes = [
                f"0x{(keymap[base + i] << 8) | keymap[base + i + 1]:04X}"
                for i in range(0, 256, 2)
            ]
            layers.append(codes)

        return {
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

    def restore(self, data: dict):
        keymap = bytes.fromhex(data["keymap_hex"])
        macros = bytes.fromhex(data["macros_hex"])
        assert len(keymap) == KEYMAP_BYTES
        assert len(macros) == MACRO_BYTES

        print("Restoring keymap (4 layers)...")
        for offset in range(0, KEYMAP_BYTES, CHUNK_SIZE):
            chunk = keymap[offset : offset + CHUNK_SIZE]
            req = bytes([0x13, (offset >> 8) & 0xFF, offset & 0xFF, len(chunk)]) + chunk
            resp = self.cmd(req)
            if resp[0] != 0x13:
                raise RuntimeError(f"Keymap write failed at offset {offset}: {resp[:8].hex()}")

        print("Restoring macros (512 bytes)...")
        for offset in range(0, MACRO_BYTES, CHUNK_SIZE):
            chunk = macros[offset : offset + CHUNK_SIZE]
            req = bytes([0x0F, (offset >> 8) & 0xFF, offset & 0xFF, len(chunk)]) + chunk
            resp = self.cmd(req)
            if resp[0] != 0x0F:
                raise RuntimeError(f"Macro write failed at offset {offset}: {resp[:8].hex()}")

        rgb = data.get("rgb", {})
        if rgb:
            print("Restoring RGB settings...")
            self.cmd(bytes([0x07, 0x03, 1, rgb["brightness"]]))
            self.cmd(bytes([0x07, 0x03, 2, rgb["effect"]]))
            self.cmd(bytes([0x07, 0x03, 3, rgb["speed"]]))
            self.cmd(bytes([0x07, 0x03, 4, rgb["hue"], rgb["saturation"]]))
            # Save lighting (cmd 0x09)
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
