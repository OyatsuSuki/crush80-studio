# Crush 80 Studio

A lightweight, pure-Rust native Windows desktop driver & custom firmware enhancement suite (**ProSuite v3**) for the **WOBKEY Crush 80 Pro Reboot (RGB)** mechanical keyboard (`VID: 0x320F`, `PID: 0x5055` USB Wired / `0x5088` 2.4G Wireless / Bluetooth BLE Battery).

轻量级纯 Rust 原生 Windows 桌面驱动与固件增强套件（**ProSuite v3**），专为 **WOBKEY Crush 80 Pro Reboot (RGB)** 打造，单文件独立运行（~3.4 MB），零浏览器/Electron 依赖。

---

## Features / 核心功能

- **88-Key F13 Digital-Twin Keymap Editor / 88 键 F13 数字孪生键位映射**
  - Full 4-layer (`L0 Win` / `L1 Win Fn` / `L2 Mac` / `L3 Mac Fn`) live Flash remapping, including the `F13` AnyKey (`Matrix R4, C12`).
  - NativeHeld **Tap-Hold** visual builder: Mod-Tap (`MT`), Layer-Tap (`LT`), Chorded Modifiers, One-Shot Mods (`OSM`), One-Shot Layers (`OSL`), Caps Word, Grave Escape.
  - One-click **Restore Key** / **Restore Layer** to factory defaults.
- **Bi-Directional Live Hardware Sync / 硬件双向实时同步**
  - Physical `Fn` shortcuts on the keyboard (backlight mode/brightness/speed, Logo RGB, Side Underglow RGB, WinLock, Win/Mac OS mode, Low-Latency ISR Debounce) reflect live in the UI within 1 second.
  - True **Rainbow vs. Static Color** toggle (`CONFIG[5]`), Logo & Side RGB controls, and **Dynamic Tap-Hold Tapping Term** (`100–300 ms`, `CONFIG[60]`).
- **Tri-Mode Support & True Battery Telemetry / 三模适配与真实电量显示**
  - **USB Wired (`0x5055`) & 2.4G Wireless (`0x5088`)**: full VIA control + real hardware battery percentage (`gp+0x10C`) in sidebar & system tray hover tooltip, with hotplug auto-reconnect.
  - **Bluetooth Mode**: automatic Windows BLE Battery Service (`DEVPKEY_Bluetooth_Battery`) detection & battery readout in sidebar and tray tooltip.
- **Visual Macro Recorder / 可视化硬件宏录制器**
  - 16 onboard macro slots (512-byte VIA/NativeHeld binary bank) supporting live keyboard recording, key chords, text strings, and millisecond delays.
- **Key Tester & Chatter Detector / 按键测试与轴体连击检测**
  - Real-time 88-key press highlight with `< 50 ms` double-click / switch chatter detection and timing log.
- **Built-In OTA Firmware Flasher / 内置图形化固件刷写与校验**
  - Pure-Rust HID `0xFFEF` OTA flasher with `KNLT` header & IEEE CRC-32 validation, live progress bar, post-reboot CRC verification, and strict USB-only safety lock.
- **Profiles & App Auto-Switch / 方案库与应用自动联动**
  - Save, import, and export `.json` profiles (including keymap, macros, RGB, and all hardware config registers) and automatically switch profiles based on the active foreground `.exe`.
- **Native Windows Integration / 原生系统集成**
  - System tray minimize-on-close, single-instance guard, optional Windows Auto-Start (`--minimized`), and live **中 / EN / 日本語** 3-language UI & keycap switching.

---

## Project Structure / 目录结构

```text
├── crush80-studio-rs/       # Pure-Rust native Windows app (eframe/egui + windows-sys)
│   ├── src/
│   │   ├── main.rs          # UI workspace, digital twin, Key Tester, Macro Recorder, OTA UI
│   │   ├── hid_win.rs       # Pure-Rust Win32 SetupAPI + Overlapped HID VIA engine
│   │   ├── ota.rs           # Pure-Rust HID 0xFFEF OTA flasher & CRC-32 validator
│   │   ├── win_sys.rs       # Single-instance mutex, HKCU Auto-Start, File Dialogs, BLE Battery
│   │   ├── tray.rs          # Native Win32 Shell_NotifyIconW system tray
│   │   ├── keycodes.rs      # QMK/VIA/WOBKEY decoder, trilingual keycaps, factory default map
│   │   ├── macro_codec.rs   # NativeHeld/VIA 512B binary macro encoder/decoder
│   │   ├── autoswitch.rs    # Foreground window watcher for per-app profile switching
│   │   └── i18n.rs          # Chinese / English / Japanese UI localization
│   └── assets/icon_64.rgba  # Embedded window icon
├── build_pro_firmware.py    # ProSuite v3 RISC-V firmware patcher + rc2emu emulator verifier
├── flash_win.py             # Standalone Python CLI OTA flasher for Windows
├── via_backup.py            # Standalone Python CLI keymap/macro/RGB backup & restore tool
├── embed_icon.py            # Windows PE icon resource embedder for Crush80_Studio.exe
├── crush80.ico / .png       # Application icon assets
└── LICENSE                  # MIT License
```

---

## Building from Source / 从源码构建

### 1. Build `Crush80_Studio.exe` (Rust)

```powershell
cd crush80-studio-rs
cargo build --release
```

Optionally embed the Windows `.ico` resource:

```powershell
uv run --with pillow python embed_icon.py
```

### 2. Build `ProSuite v3` Firmware (Optional)

Clone the upstream [wobkey-crush80-reboot-pro-firmware](https://github.com/tarzst/wobkey-crush80-reboot-pro-firmware) repository alongside `build_pro_firmware.py`, then run:

```powershell
uv run python build_pro_firmware.py
```

The builder patches `Crush80-RGB-0108-NativeHeld-20261101.bin` and runs full RISC-V emulation verification (`rc2emu`) before writing `Crush80-RGB-0108-NativeHeld-v130-ProSuite.bin` (OTA CRC `0xC03D5821`).

---

## Acknowledgments / 致谢

- Firmware base (`NativeHeld v1.3.0`) and `rc2emu` RISC-V emulator by **[tarzst/wobkey-crush80-reboot-pro-firmware](https://github.com/tarzst/wobkey-crush80-reboot-pro-firmware)**.
- Built with [eframe / egui](https://github.com/emilk/egui) and [windows-sys](https://github.com/microsoft/windows-rs).
