#!/usr/bin/env python3
"""Build Windows icon assets from crush80_icon.png and embed them into Crush80_Studio.exe:
  1. Multi-size Windows .ico (crush80.ico: 256, 64, 48, 32, 16)
  2. Raw 64x64 RGBA bytes (crush80-studio-rs/assets/icon_64.rgba) for Rust ViewportBuilder
  3. Embed RT_ICON + RT_GROUP_ICON directly into Crush80_Studio.exe via Win32 kernel32
"""
import ctypes
from ctypes import wintypes
import io
from pathlib import Path
import struct
import time
from PIL import Image

ROOT = Path(__file__).resolve().parent
PNG_OUT = ROOT / "crush80_icon.png"
ICO_OUT = ROOT / "crush80.ico"
RGBA64_OUT = ROOT / "crush80-studio-rs" / "assets" / "icon_64.rgba"
EXE_PATH = ROOT / "Crush80_Studio.exe"


def build_ico_from_master(master: Image.Image, sizes=(256, 64, 48, 32, 16)):
    images = []
    for sz in sizes:
        resized = master.resize((sz, sz), Image.Resampling.LANCZOS)
        buf = io.BytesIO()
        resized.save(buf, format="PNG", optimize=True)
        images.append((sz, buf.getvalue()))

    header = struct.pack("<HHH", 0, 1, len(images))
    entries = bytearray()
    offset = 6 + 16 * len(images)
    for sz, png in images:
        dim = 0 if sz >= 256 else sz
        entries.extend(struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), offset))
        offset += len(png)
    return header + bytes(entries) + b"".join(p for _, p in images), images


def embed_into_exe(exe_path: Path, images: list[tuple[int, bytes]]):
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.BeginUpdateResourceW.argtypes = [wintypes.LPCWSTR, wintypes.BOOL]
    k32.BeginUpdateResourceW.restype = wintypes.HANDLE
    k32.UpdateResourceW.argtypes = [
        wintypes.HANDLE,
        wintypes.LPCWSTR,
        wintypes.LPCWSTR,
        wintypes.WORD,
        wintypes.LPVOID,
        wintypes.DWORD,
    ]
    k32.UpdateResourceW.restype = wintypes.BOOL
    k32.EndUpdateResourceW.argtypes = [wintypes.HANDLE, wintypes.BOOL]
    k32.EndUpdateResourceW.restype = wintypes.BOOL

    RT_ICON = ctypes.cast(3, wintypes.LPCWSTR)
    RT_GROUP_ICON = ctypes.cast(14, wintypes.LPCWSTR)
    LANG_NEUTRAL = 0

    last_err = 0
    for attempt in range(3):
        h = k32.BeginUpdateResourceW(str(exe_path), False)
        if not h:
            last_err = ctypes.get_last_error()
            if attempt < 2:
                time.sleep(0.6)
                continue
            raise OSError(f"BeginUpdateResourceW failed after 3 attempts: winerr={last_err}")
        try:
            grp_header = struct.pack("<HHH", 0, 1, len(images))
            grp_entries = bytearray()
            for icon_id, (sz, png) in enumerate(images, start=1):
                res_name = ctypes.cast(icon_id, wintypes.LPCWSTR)
                buf = ctypes.create_string_buffer(png)
                if not k32.UpdateResourceW(h, RT_ICON, res_name, LANG_NEUTRAL, buf, len(png)):
                    raise OSError(
                        f"UpdateResourceW(RT_ICON {icon_id}) failed: winerr={ctypes.get_last_error()}"
                    )
                dim = 0 if sz >= 256 else sz
                grp_entries.extend(struct.pack("<BBBBHHIH", dim, dim, 0, 0, 1, 32, len(png), icon_id))
            grp_data = grp_header + bytes(grp_entries)
            grp_buf = ctypes.create_string_buffer(grp_data)
            if not k32.UpdateResourceW(
                h, RT_GROUP_ICON, ctypes.cast(1, wintypes.LPCWSTR), LANG_NEUTRAL, grp_buf, len(grp_data)
            ):
                raise OSError(
                    f"UpdateResourceW(RT_GROUP_ICON) failed: winerr={ctypes.get_last_error()}"
                )
            if k32.EndUpdateResourceW(h, False):
                return
            last_err = ctypes.get_last_error()
            if attempt < 2:
                time.sleep(0.8)
                continue
            raise OSError(f"EndUpdateResourceW failed: winerr={last_err}")
        except Exception:
            k32.EndUpdateResourceW(h, True)
            if attempt < 2:
                time.sleep(0.8)
                continue
            raise


if __name__ == "__main__":
    master = Image.open(PNG_OUT).convert("RGBA")
    RGBA64_OUT.parent.mkdir(parents=True, exist_ok=True)
    RGBA64_OUT.write_bytes(master.resize((64, 64), Image.Resampling.LANCZOS).tobytes())
    ico_bytes, images = build_ico_from_master(master)
    ICO_OUT.write_bytes(ico_bytes)
    print(f"Prepared {ICO_OUT} ({len(ico_bytes)} bytes) and {RGBA64_OUT} from {PNG_OUT}")
    if EXE_PATH.exists():
        embed_into_exe(EXE_PATH, images)
        print(f"Embedded studio icon into {EXE_PATH}!")
