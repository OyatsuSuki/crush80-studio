# Crush 80 Studio & Ember Full Audit Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (chosen: Native mode — execute task-by-task in this session, then run a final independent read-only code review on the whole branch). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve all 7 Critical, 11 Important, and 10 Minor findings from the `v1.2.1` full codebase audit across the Rust Windows driver, Ember RISC-V firmware builder, Python utilities, and GitHub Actions CI/Release workflow.

**Architecture:** Harden the Win32 HID/OTA/Tray/Storage foundation first (eliminating UB, OTA brick risks, concurrency races, and path traversal), then decouple all blocking HID I/O into a dedicated coalescing `HidWorker` behind `trait HidTransport`, modularize the 3,110-line `main.rs` into focused `src/ui/*` modules with full error surfacing and native Win32 key-state capture, apply the zero-byte `0xFD` GET/SET firmware fix with `rc2emu` verification, and lock down Python tooling + CI with automated tests and `clippy`.

**Tech Stack:** Rust (`eframe`/`egui` 0.30, `windows-sys` 0.59, `serde`/`serde_json`), Python 3 (`hidapi`, `Pillow`, `rc2emu`), RISC-V RV32IMC binary patching, GitHub Actions (`windows-latest`).

**Spec:** Full Read-Only Code Review Report (`adc9758e..affe9826`, 7 Critical / 11 Important / 10 Minor) + User Architectural Decisions (A1–A4):
- **A1 (Data Dir):** Default `%APPDATA%\Crush80Studio`; auto-switch to portable mode if `exe_dir/studio/` or `exe_dir/portable.txt` exists and is writable. Remove hardcoded developer paths.
- **A2 (OTA Policy):** Whitelist-first + explicit pre-flash confirmation dialog (`current_crc -> target_crc`); hard-lock RGB flashing when Lite Non-RGB matrix is detected; unknown source CRC or unknown `.bin` blocked unless user checks explicit high-risk acknowledgement.
- **A3 (Ember Firmware):** Apply zero-byte `0xFD` GET/SET fix in `build_pro_firmware.py`, verify via `rc2emu`, emit `Crush80-RGB-Ember-v1.3.0.bin` (do NOT auto-flash user's keyboard).
- **A4 (Execution):** Native mode (`executing-plans`) with TDD + per-task verification, followed by a final fresh read-only code review subagent.

## Global Constraints

- Pure Rust Windows binary (`crush80-studio.exe`, x86_64 Windows GNU/MSVC) with zero new external crates required for offline builds (use pure-Rust SHA-256 in `ota.rs`).
- Firmware display name stays purely **`Ember`** (version number only in `.bin` filename and Studio app titlebar).
- Never touch OEM bootloader, OTA stack, RGB startup, sleep hooks, or `0x12F00`; every firmware change must pass full `rc2emu` verification (`coldboot`, `0x0E`, `0x12`, Tap-Hold, `0xFD`).
- All UI copy remains concise and trilingual (`Zh` / `En` / `Ja`) without `"中文 (English)"` doc-style parentheses.
- Every task leaves `cargo test` and `cargo clippy -- -D warnings` (once enabled) passing cleanly.

## Review Focus

The 5 highest-impact failure modes pinned by explicit tests in their owning tasks:
1. **HID timeout/disconnect during write or OTA:** `write_report`/`read_report` must wait via `CancelIoEx` + `GetOverlappedResult(..., TRUE)` and reject `GetOverlappedResult == 0` or partial writes (Task 1).
2. **Crush 80 Lite with remapped Spacebar (`LT(1, Space)`) attempting Pro RGB OTA:** Edition detection must still recognize Lite Non-RGB and `ota.rs` policy must hard-reject RGB flashing (Tasks 2 & 3).
3. **Path traversal / reserved Windows filename in Profile or AutoSwitch (`..\autoswitch`, `CON`, `C:\evil`):** Storage sanitization must reject before any filesystem call (Task 4).
4. **Windows `Ctrl+C` macro recording & Explorer restart while minimized to tray:** Macro recorder must emit `LCtrl` only (never `LWin`), and tray must re-register on `TaskbarCreated` without panicking across FFI (Tasks 4 & 6).
5. **Non-BMP emoji in tray tooltip, read-only `Program Files` install dir, or non-`C:` `WINDIR`:** UTF-16 truncation must not split surrogate pairs, data dir must fall back to `%APPDATA%\Crush80Studio`, and fonts must resolve via `WINDIR` (Tasks 4 & 6).

---

## File Structure Map

- **Modify:**
  - `crush80-studio-rs/Cargo.toml` — bump to `1.3.0` on release readiness.
  - `crush80-studio-rs/src/hid_win.rs` — safe Win32 Overlapped I/O & aligned SetupAPI, `trait HidTransport`, global `is_ota_busy()` gate, `FullSnapshot.edition`, response opcode validation, 2.4G OTA skip, unit tests.
  - `crush80-studio-rs/src/ota.rs` — pure-Rust SHA-256, `OtaFlashPolicy` & source-CRC/Lite hard-lock enforcement, strict `Result(code != 0)` handling, same-CRC disconnect verification, unit tests.
  - `crush80-studio-rs/src/keycodes.rs` — strengthened `detect_edition_from_keymap`, Lite `(5,5) -> 0x0000` fix, localized basic key labels, dead code removal, unit tests.
  - `crush80-studio-rs/src/macro_codec.rs` — reject `0x00` keycode & non-printable control bytes, `validate_macro_text`, expanded unit tests.
  - `crush80-studio-rs/src/win_sys.rs` — `Local\` mutex, PID-verified `EnumWindows` finder, `is_autostart_enabled` path match, char-safe `copy_utf16_truncated`, Win32 `GetAsyncKeyState` snapshot helper, unit tests.
  - `crush80-studio-rs/src/tray.rs` — PID-verified main window capture, `TaskbarCreated` recovery, `WM_NULL` menu dismiss, panic-free FFI callback, OTA-busy quit guard.
  - `crush80-studio-rs/src/autoswitch.rs` — `is_ota_busy()` guard, sanitized profile lookup, RAM-only snapshot apply, update `active_rule_id` only on success, pure matching tests.
  - `crush80-studio-rs/src/i18n.rs` — strings for OTA confirmation/risk modal, error toasts, Lite subtitle, Bluetooth OTA label.
  - `crush80-studio-rs/src/main.rs` — slim app coordinator (~350–500 lines) wiring `HidWorker`, `storage`, and `ui::*`.
  - `build_pro_firmware.py` — `0xFD` jump to `CONFIG_HANDLER + 2`, emit `Crush80-RGB-Ember-v1.3.0.bin`, expanded `rc2emu` tests, updated docstrings.
  - `embed_icon.py` — `WinDLL("kernel32", use_last_error=True)`, raise on retry exhaustion, remove dead code.
  - `via_backup.py` — prefer USB `0x5055`, verify write & resp len, replace `assert` with `ValueError`, backup/restore `rainbow` + `hw`.
  - `.github/workflows/ci-release.yml` — least-privilege permissions, pinned deps, `cargo test` + `clippy`, `SHA256SUMS.txt`.
- **Create:**
  - `crush80-studio-rs/src/storage.rs` — `sanitize_profile_name`, `resolve_profile_path`, `resolve_data_dir` (`%APPDATA%` + portable mode) + unit tests.
  - `crush80-studio-rs/src/hid_worker.rs` — background HID command/event worker with RGB coalescing/throttling + tray-aware live polling with locked TOCTOU check.
  - `crush80-studio-rs/src/ui/{mod,theme,widgets,keymap,rgb,hardware,macros,profiles,autoswitch,tester,ota}.rs` — modularized UI pages & shared keyboard/palette widgets.

---

### Task 1: Win32 HID FFI Safety, Protocol Validation & `HidTransport` Abstraction

**Covers:** Critical #1, Critical #2, Critical #5 (HID side), Important #2, Important #5, Important #11 (HID/protocol tests), Minor #3, Minor #9.

**Files:**
- Modify: `crush80-studio-rs/src/hid_win.rs`
- Test: inline `#[cfg(test)] mod tests` in `crush80-studio-rs/src/hid_win.rs`

**Interfaces:**
- Consumes: `crate::keycodes::BoardEdition`, `crate::ota::known_firmware_name`
- Produces:
  - `pub(crate) trait HidTransport { fn write_report(&self, data: &[u8]) -> Result<(), String>; fn read_report(&self, timeout_ms: u32) -> Result<Vec<u8>, String>; }`
  - `FullSnapshot { timestamp, keymap_hex, macros_hex, rgb, hw, #[serde(default)] pub edition: Option<BoardEdition> }`
  - `Crush80Hid::apply_snapshot_checked(&self, snap: &FullSnapshot, current_edition: Option<BoardEdition>, save_to_flash: bool) -> Result<(), String>`
  - `Crush80Hid::export_snapshot(&self, timestamp: String, edition: BoardEdition) -> Result<FullSnapshot, String>`

- [ ] **Step 1: Write failing unit tests in `hid_win.rs`**
  Add `#[cfg(test)] mod tests` with a `MockTransport` testing:
  1. `test_via_cmd_rejects_mismatched_opcode` — expects `Err` when request `[0x05, ...]` gets response `[0x08, ...]`.
  2. `test_apply_snapshot_rejects_cross_edition` — `FullSnapshot` with `edition: Some(BoardEdition::ProRgb)` applied to `Some(BoardEdition::LiteNoRgb)` returns `Err`.
  3. `test_apply_snapshot_ram_only_skips_save_0x09` — when `save_to_flash == false`, `MockTransport` records zero `[0x09]` packets and clamps `logo_mode`/`side_mode` to `<= 3`.
  4. `test_snapshot_backward_compat_without_edition` — JSON missing `edition` and `hw` deserializes with `edition == None`.
  5. `test_hex_roundtrip_and_invalid_hex` — verifies `to_hex`/`from_hex` error cases.
  6. `test_ota_busy_blocks_all_via_ops` — with `try_begin_ota()` active, `read_keymap`, `set_key`, `set_rgb_partial`, `set_config_byte`, `write_macro_buffer`, `apply_snapshot_checked` all immediately return `Err` mentioning OTA.

- [ ] **Step 2: Run tests to verify failure**
  Run: `cargo test hid_win::tests`
  Expected: FAIL (missing `HidTransport`, `apply_snapshot_checked`, `edition` field, etc.).

- [ ] **Step 3: Implement Win32 FFI fixes & protocol hardening in `hid_win.rs`**
  - In `find_hid_devices`: allocate `vec![0u32; (req_size as usize + 3) / 4]` for 4-byte alignment; verify `req_size as usize >= size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>()`; bound UTF-16 device path scan by `max_u16 = (req_size as usize - offset_of_device_path) / 2` using `std::slice::from_raw_parts`.
  - In `WinHidHandle::write_report` & `read_report`: check `ev.is_null()`; on timeout call `CancelIoEx(self.handle, &ov)` followed by `GetOverlappedResult(self.handle, &ov, &mut transferred, 1 /* TRUE */)` BEFORE `CloseHandle(ev)`; on normal completion verify `GetOverlappedResult(self.handle, &ov, &mut transferred, 0) != 0` and for writes verify `written as usize == buf.len()`.
  - Implement `HidTransport` for `WinHidHandle`; refactor `via_cmd`, `read_keymap_with`, `read_hw_config_with`, `apply_snapshot_with` to accept `&impl HidTransport`.
  - In `via_cmd`: verify `!payload.is_empty()` and `resp[1] == payload[0]`.
  - In `get_status()`: only query `USAGE_OTA` when `m.pid == PID_USB`, and reuse `crate::ota::known_firmware_name(crc)`.
  - Add `if self.is_ota_busy() { return Err("OTA 刷写进行中，已暂停设备操作".into()); }` at the top of every non-OTA public method (`read_keymap`, `set_key`, `read_rgb`, `set_rgb_partial`, `read_hw_config`, `set_config_byte`, `trigger_led_feedback`, `read_macros`, `write_macro_buffer`, `export_snapshot`, `apply_snapshot_checked`).
  - Clamp `hw.logo_mode.min(3)` and `hw.side_mode.min(3)` and only write extended `CONFIG[21..60]` when `hw.prosuite_supported` (or live probe confirms Ember); remove unused `write_macros` and dead `transport`/`via_protocol` warnings (or expose only used fields).

- [ ] **Step 4: Run tests to verify pass**
  Run: `cargo test hid_win::tests`
  Expected: All 6 tests PASS.

- [ ] **Step 5: Commit**
  `git commit -m "fix(hid): eliminate Win32 Overlapped UAF/alignment UB, add HidTransport and OTA-busy gates"`

---

### Task 2: Hardened OTA Engine & Anti-Brick Core

**Covers:** Critical #3, Critical #4, Important #11 (OTA unit tests).

**Files:**
- Modify: `crush80-studio-rs/src/ota.rs`
- Test: inline `#[cfg(test)] mod tests` in `crush80-studio-rs/src/ota.rs`

**Interfaces:**
- Consumes: `crate::hid_win::{Crush80Hid, HidTransport}`, `crate::keycodes::BoardEdition`
- Produces:
  - `pub struct OtaFlashPolicy { pub detected_edition: Option<BoardEdition>, pub allow_unknown_source_crc: bool, pub allow_unknown_image: bool }`
  - `pub fn sha256_hex(data: &[u8]) -> String`
  - `pub fn is_known_pro_rgb_source_crc(crc: u32) -> bool`
  - `pub fn check_ota_policy(info: &FirmwareInfo, current_crc: u32, policy: &OtaFlashPolicy) -> Result<(), String>`
  - `pub fn flash_firmware_async(hid: Arc<Crush80Hid>, image: Vec<u8>, policy: OtaFlashPolicy, progress: Arc<Mutex<OtaProgress>>)`

- [ ] **Step 1: Write failing unit tests in `ota.rs`**
  1. `test_crc32_and_crc16_known_vectors` (`b"123456789"` -> `0xCBF43926` / `0x4B37`).
  2. `test_sha256_known_vectors` (`b""` & `b"abc"` standard NIST vectors).
  3. `test_validate_firmware_image_edge_cases` (synthetic valid 124,436B image with `KNLT`, internal size, trailing `~crc32`; reject wrong len, bad magic, wrong internal size, corrupted byte).
  4. `test_ota_packet_builders_and_parser` (`build_ota_packet`, `build_data_packet` block count/padding/CRC16, `build_end_packet` two's complement `last` & `-last`, `parse_ota_response` for `Info`, `Result(0)`, `Result(5)`, `OtaAck`, truncated/malformed).
  5. `test_check_ota_policy_anti_brick` — hard-rejects `detected_edition == Some(BoardEdition::LiteNoRgb)` even if overrides are true; rejects unknown `current_crc` unless `allow_unknown_source_crc == true`; rejects `!info.is_known` unless `allow_unknown_image == true`; accepts known Pro RGB source + known image.
  6. `test_end_ota_explicit_nonzero_result_fails_immediately` — verifies explicit `OtaResponse::Result(code != 0)` does NOT enter post-reboot polling (`reached_end == false`).
  7. `test_same_crc_reflash_requires_reboot_or_explicit_ack` — verifies helper logic does not declare success on stale pre-reboot CRC when `expected_crc == current_crc` and no `Result(0)` was received.

- [ ] **Step 2: Run tests to verify failure**
  Run: `cargo test ota::tests`
  Expected: FAIL (missing `sha256_hex`, `OtaFlashPolicy`, `check_ota_policy`).

- [ ] **Step 3: Implement OTA hardening in `ota.rs`**
  - Add compact pure-Rust `sha256_hex(&[u8]) -> String` and extend `FirmwareInfo` with `pub sha256: String` + known SHA-256 verification for official/Ember releases (including `Crush80-RGB-Ember-v1.2.0.bin` and upcoming `v1.3.0`).
  - Add `KNOWN_PRO_RGB_SOURCE_CRCS` matching `flash_win.py` + Ember CRCs, `is_known_pro_rgb_source_crc`, and `check_ota_policy`.
  - In `flash_firmware_async`:
    - Call `check_ota_policy(&info, current_crc, &policy)` right after `query_ota_info(&handle)` and before sending Start OTA.
    - Replace `progress.lock().unwrap()` with safe `if let Ok(mut p) = progress.lock()`.
    - In End OTA phase: `OtaResponse::Result(0)` returns `Ok(true)` (`got_explicit_ack = true`); explicit `OtaResponse::Result(code)` returns `Err((false, ...))` (immediate failure, never enters 20s poll!); ONLY `write_report`/`read_report` transport error after end packet returns `Err((true, ...))` to enter reboot verification.
    - In post-restart polling: if `expected_crc == current_crc && !got_explicit_ack`, require observing at least one disconnect/open error (`saw_disconnect = true`) before accepting `crc == expected_crc`.

- [ ] **Step 4: Run tests to verify pass**
  Run: `cargo test ota::tests`
  Expected: All 7 tests PASS.

- [ ] **Step 5: Commit**
  `git commit -m "fix(ota): enforce core anti-brick policy, explicit error handling, and same-CRC verification"`

---

### Task 3: Keycodes, Edition Detection & Macro Codec Safety

**Covers:** Critical #3 (Lite detection resilience), Important #7, Important #11 (keycodes & macro_codec tests), Minor #2 (localized basic key names), Minor #9, Minor #10.

**Files:**
- Modify: `crush80-studio-rs/src/keycodes.rs`
- Modify: `crush80-studio-rs/src/macro_codec.rs`
- Test: inline `#[cfg(test)] mod tests` in both files

**Interfaces:**
- Produces:
  - `macro_codec::validate_macro_text(s: &str) -> Result<(), String>`
  - `keycodes::basic_keycode_label(code: u16, lang: Lang) -> String` (for localized Basic Key picker in Task 6)

- [ ] **Step 1: Write failing unit tests in `keycodes.rs` and `macro_codec.rs`**
  - In `keycodes.rs`:
    1. `test_detect_edition_pro_and_lite_stock`
    2. `test_detect_edition_lite_with_remapped_lt_space` — Lite board where `(5,6)` is `0x412C` (`LT(1, Space)`) and `(4,12)` is `0x00E5` still detects `Some(BoardEdition::LiteNoRgb)`.
    3. `test_lite_factory_default_zeroes_r5c5_and_swaps_4_coords` — asserts `(5,5) == 0x0000`, `(5,6) == 0x002C`, `(4,12) == 0x00E5`, `(4,13) == 0x0068`, `(5,8) == 0x0065`, `(5,11) == 0x0000`.
    4. `test_layouts_within_matrix_bounds` — every `PhysicalKey` in both layouts has `r < 8 && c < 16`.
  - In `macro_codec.rs`:
    1. `test_encode_rejects_zero_keycode_in_tap_down_up` — `Tap(0)`, `Down(0)`, `Up(0)` all return `Err`.
    2. `test_encode_rejects_control_chars_and_non_ascii` — `\n`, `\x07`, `\x7F`, `"中文"` all return `Err`; printable `0x20..=0x7E` succeeds.
    3. `test_validate_macro_text_matches_encoder`.

- [ ] **Step 2: Run tests to verify failure**
  Run: `cargo test keycodes::tests macro_codec::tests`
  Expected: FAIL on Lite `(5,5)`, `LT(1, Space)` Lite detection, `Tap(0)` rejection, and control-char rejection.

- [ ] **Step 3: Implement fixes in `keycodes.rs` and `macro_codec.rs`**
  - `keycodes.rs`:
    - In `factory_default_key_for(LiteNoRgb, ...)`: return `0x0000` for `(5, 5)` as well as `(5, 11)`.
    - In `detect_edition_from_keymap`: match space-like keycodes via helper `is_space_like(c) = (c == 0x002C) || ((0x4000..=0x7FFF).contains(&c) && (c & 0x00FF) == 0x2C)`, plus `(5,8) == 0x0065`/`(5,11) == 0x0000` tie-breakers so Lite boards with remapped `LT(1, Space)` still score strongly as `LiteNoRgb`.
    - Add `basic_keycode_label(code: u16, lang: Lang) -> String` reusing `decode_keycode_for_lang`.
    - Remove dead `RGB_EFFECTS`, `decode_keycode`, `factory_default_key`.
  - `macro_codec.rs`:
    - Reject `kc == 0x00` in `Tap`/`Down`/`Up` inside `encode_macro_buffer`.
    - Restrict `MacroStep::Text` bytes to printable ASCII `0x20..=0x7E` and expose `pub fn validate_macro_text(s: &str) -> Result<(), String>`.

- [ ] **Step 4: Run tests to verify pass**
  Run: `cargo test keycodes::tests macro_codec::tests`
  Expected: All tests PASS.

- [ ] **Step 5: Commit**
  `git commit -m "fix(keycodes,macro): harden Lite matrix detection/defaults and forbid 0x00/control macro bytes"`

---

### Task 4: Storage Security, Win32 System, Tray & AutoSwitch Hardening

**Covers:** Critical #5 (Tray & AutoSwitch sides), Critical #7, Important #4, Important #8, Important #9 (autostart & autoswitch), Important #11 (storage/autoswitch/win_sys tests), Minor #5.

**Files:**
- Create: `crush80-studio-rs/src/storage.rs`
- Modify: `crush80-studio-rs/src/win_sys.rs`
- Modify: `crush80-studio-rs/src/tray.rs`
- Modify: `crush80-studio-rs/src/autoswitch.rs`
- Modify: `crush80-studio-rs/src/main.rs` (declare `mod storage;` only for compilation)

**Interfaces:**
- Produces:
  - `storage::sanitize_profile_name(name: &str) -> Result<String, String>`
  - `storage::resolve_profile_path(profiles_dir: &Path, name: &str) -> Result<PathBuf, String>`
  - `storage::resolve_data_dir() -> PathBuf` (and testable `resolve_data_dir_with(exe_dir: Option<&Path>, appdata: Option<&Path>) -> PathBuf`)
  - `win_sys::copy_utf16_truncated(dst: &mut [u16], text: &str)`
  - `win_sys::find_own_main_window(title_prefix: &str, exclude_hwnd: HWND) -> HWND`
  - `win_sys::poll_win32_special_keys() -> [bool; 14]` (for accurate `LCtrl/RCtrl/LShift/RShift/LAlt/RAlt/LWin/RWin/CapsLock/PrtSc/ScrLk/Pause/Menu/F13` in Task 6)
  - `tray::TrayState::new(hid: Arc<Crush80Hid>) -> Arc<Self>` + `pub window_hidden: Arc<AtomicBool>`
  - `autoswitch::select_transition(cfg: &AutoSwitchConfig, active_rule_id: Option<&str>, exe: &str) -> SwitchTransition`

- [ ] **Step 1: Write failing unit tests in `storage.rs`, `win_sys.rs`, and `autoswitch.rs`**
  - `storage.rs`:
    1. `test_sanitize_profile_name_blocks_traversal_and_reserved` (`"../autoswitch"`, `"a\\b"`, `"C:\\foo"`, `"CON"`, `"nul"`, `"a:b"`, `"foo."`, `""`, `"   "` all `Err`; `"游戏配置 1"`, `"Gaming_Pro-v2"` `Ok`).
    2. `test_resolve_profile_path_stays_inside_profiles_dir`.
    3. `test_resolve_data_dir_prefers_portable_studio_or_marker_else_appdata`.
  - `win_sys.rs`:
    1. `test_copy_utf16_truncated_preserves_surrogate_pairs` (emoji `"🔥".repeat(10)` into `[0u16; 6]` writes exact 2 emojis = 4 code units + NUL, never lone high surrogate).
    2. `test_build_filter_wide_double_null_terminated`.
  - `autoswitch.rs`:
    1. `test_select_transition_matches_case_insensitive_csv_and_reverts` (verifies matching, no-op when already active, revert to `default_rgb` when leaving matched exe).

- [ ] **Step 2: Run tests to verify failure**
  Run: `cargo test storage::tests win_sys::tests autoswitch::tests`
  Expected: FAIL.

- [ ] **Step 3: Implement `storage.rs`, `win_sys.rs`, `tray.rs`, and `autoswitch.rs` fixes**
  - `storage.rs`: implement `sanitize_profile_name`, `resolve_profile_path` (verifying `path.parent() == Some(profiles_dir)`), and `resolve_data_dir` (checks `exe_dir/studio` or `exe_dir/portable.txt` writability first -> portable, else `%APPDATA%\Crush80Studio`).
  - `win_sys.rs`:
    - Change `SINGLE_INSTANCE_MUTEX_NAME` to `"Local\\Crush80Studio_SingleInstance_v1"`.
    - Add `copy_utf16_truncated(dst, text)` iterating `char`s by `c.len_utf16()` (use in `save_file_dialog`).
    - Add `find_own_main_window` using `EnumWindows` + `GetWindowThreadProcessId == GetCurrentProcessId()` + `GetWindowTextW` check; use in `find_owner_hwnd()`.
    - Fix `is_autostart_enabled()` to read the actual `REG_SZ` UTF-16 value and compare normalized exe path against `std::env::current_exe()`.
    - Add `poll_win32_special_keys()` using `GetAsyncKeyState` for left/right modifiers & special keys (`VK_LCONTROL`, `VK_RCONTROL`, `VK_LSHIFT`, `VK_RSHIFT`, `VK_LMENU`, `VK_RMENU`, `VK_LWIN`, `VK_RWIN`, `VK_CAPITAL`, `VK_SNAPSHOT`, `VK_SCROLL`, `VK_PAUSE`, `VK_APPS`, `VK_F13`).
  - `tray.rs`:
    - Add `hid: Arc<Crush80Hid>` and `pub window_hidden: Arc<AtomicBool>` to `TrayState`.
    - Use `win_sys::find_own_main_window` & `win_sys::copy_utf16_truncated`.
    - Register `WM_TASKBAR_CREATED = RegisterWindowMessageW("TaskbarCreated")` and re-run `NIM_ADD` when received.
    - Send `PostMessageW(hwnd, WM_NULL, 0, 0)` after `TrackPopupMenu`.
    - Replace ALL `.lock().unwrap()` inside `tray_wnd_proc` with `if let Ok(...)`.
    - In `CMD_QUIT`: if `st.hid.is_ota_busy()`, refuse exit and show Win32 `MessageBoxW` warning instead of `std::process::exit(0)`.
    - Update `window_hidden` atomic inside `hide_main_window` / `restore_main_window`.
  - `autoswitch.rs`:
    - Extract pure `select_transition`.
    - Check `if hid.is_ota_busy() { continue; }`.
    - Use `storage::resolve_profile_path` and `hid.apply_snapshot_checked(&snap, None, false)` (RAM-only, zero Flash wear!).
    - Only commit `st.active_rule_id` AFTER the HID call succeeds (`Ok(())`), so disconnected/sleeping boards retry automatically on reconnect.
    - Return `Result<(), String>` from `save_config` instead of swallowing I/O errors.

- [ ] **Step 4: Run tests to verify pass**
  Run: `cargo test storage::tests win_sys::tests autoswitch::tests`
  Expected: All tests PASS.

- [ ] **Step 5: Commit**
  `git commit -m "fix(sys,tray,storage,autoswitch): prevent path traversal, harden Win32 tray/mutex, add APPDATA+portable storage"`

---

### Task 5: Non-Blocking `HidWorker` & Optimized Live Sync

**Covers:** Important #1, Important #3, Important #9 (centralized async error reporting).

**Files:**
- Create: `crush80-studio-rs/src/hid_worker.rs`
- Modify: `crush80-studio-rs/src/main.rs` (declare `mod hid_worker;`)

**Interfaces:**
- Consumes: `crate::hid_win::*`, `crate::keycodes::BoardEdition`, `crate::win_sys::query_bluetooth_battery`
- Produces:
  - `pub enum HidCmd { RefreshAll, SetKey { layer, row, col, keycode }, RestoreLayer { layer, edition, keys: Vec<(u8, u8, u16)> }, SetRgb { brightness, effect, speed, color, rainbow, save }, SetConfigByte { id, val, save }, TriggerLedFeedback { count, r, g, b }, WriteMacroBuffer { raw: Vec<u8> }, ExportSnapshot { path: PathBuf, edition: BoardEdition, name_for_toast: String }, ApplySnapshot { snap: FullSnapshot, edition: Option<BoardEdition>, save_to_flash: bool, label: String } }`
  - `pub enum HidEvent { FullRefresh { status, layers, macros_raw, rgb, hw, bt_info, warnings: Vec<String> }, LiveUpdate { pid, rgb, hw, bt_info, connected }, KeyUpdated { layer, row, col, keycode }, LayerRestored { layer, count }, SnapshotExported { path: PathBuf, label: String }, SnapshotApplied { label: String }, ToastOk(String), ToastErr(String) }`
  - `pub struct HidWorkerHandle { pub tx: mpsc::Sender<HidCmd>, pub rx: mpsc::Receiver<HidEvent>, last_ui_edit: Arc<Mutex<Instant>> }` with `send(&self, cmd)` and `mark_ui_edit(&self)`

- [ ] **Step 1: Write failing unit tests in `hid_worker.rs`**
  1. `test_coalesce_rgb_commands_merges_pending_slider_burst` — queue 10 `SetRgb` commands; coalescer collapses them to 1 merged command preserving the latest brightness/effect/speed/color/rainbow and `save |= next.save`.
  2. `test_live_sync_skips_stale_publish_when_ui_edited_mid_poll` — verifies locked re-check of `last_ui_edit` drops stale poll results (TOCTOU fix).
  3. `test_poll_interval_slows_when_hidden_to_tray` — visible = 1s, hidden = 8s, BLE PnP throttle = 12s.

- [ ] **Step 2: Run tests to verify failure**
  Run: `cargo test hid_worker::tests`
  Expected: FAIL.

- [ ] **Step 3: Implement `hid_worker.rs`**
  - Dedicated command worker thread owning `Arc<Crush80Hid>`: drains pending queue, coalesces adjacent `SetRgb` / `SetConfigByte(same id)` commands with 30ms debounce window, executes HID ops off the UI thread, and emits `HidEvent` + `ctx.request_repaint()`.
  - Dedicated live-sync thread checking `window_hidden` (1s visible / 8s hidden), skipping while `hid.is_ota_busy()`, throttling `query_bluetooth_battery()` to 12s (and only when USB/2.4G is offline), and re-checking `last_ui_edit.elapsed() >= 1200ms` **before publishing** `HidEvent::LiveUpdate`.
  - `RefreshAll` collects partial errors into `warnings` instead of silently swallowing keymap/macro read failures.

- [ ] **Step 4: Run tests to verify pass**
  Run: `cargo test hid_worker::tests`
  Expected: All 3 tests PASS.

- [ ] **Step 5: Commit**
  `git commit -m "feat(hid_worker): move all HID I/O off UI thread with RGB coalescing and tray-aware live sync"`

---

### Task 6: Modularize `main.rs` into `src/ui/*` & Fix All UI / Input / Error-Handling Issues

**Covers:** Critical #3 (UI OTA confirmation modal & risk checkbox), Critical #6 (`Modifiers::command` -> Win32 key polling), Important #1 (zero blocking UI I/O), Important #9 (eliminate all `let _ = ...` & disable unavailable controls), Minor #1, Minor #2, Minor #4, Minor #6, Minor #7.

**Files:**
- Create: `crush80-studio-rs/src/ui/{mod,theme,widgets,keymap,rgb,hardware,macros,profiles,autoswitch,tester,ota}.rs`
- Modify: `crush80-studio-rs/src/main.rs`
- Modify: `crush80-studio-rs/src/i18n.rs`

**Interfaces:**
- Consumes: `crate::hid_worker::*`, `crate::storage::*`, `crate::ota::{OtaFlashPolicy, ...}`, `crate::macro_codec::validate_macro_text`, `crate::keycodes::basic_keycode_label`, `crate::win_sys::poll_win32_special_keys`
- Produces: Clean `Crush80App` coordinating modular `ui::*` views with zero blocking HID calls on UI thread.

- [ ] **Step 1: Add missing i18n strings in `i18n.rs`**
  Add trilingual (`Zh`/`En`/`Ja`) copy for:
  - OTA confirmation dialog (title, current -> target CRC summary, Lite hard-lock message, high-risk unknown CRC/image checkbox, confirm/cancel).
  - Bluetooth transport label on OTA tab (Minor #1).
  - Lite edition sidebar subtitle (`"LITE · NO-RGB"` vs `"PRO REBOOT"`, Minor #7).
  - Profile/macro/autoswitch validation & disk error toast prefixes.

- [ ] **Step 2: Create `src/ui/` modules and refactor `main.rs`**
  - `ui/theme.rs` & `ui/widgets.rs`: shared dark industrial theme, unified `draw_keyboard_twin` (shared by Keymap & Tester), shared 8-color `LOGO_SIDE_PALETTE` widget (eliminating Minor #4 duplication).
  - `ui/keymap.rs`: route key writes & layer restore through `HidWorkerHandle`; use `basic_keycode_label(code, self.lang)` for localized Basic Key picker & search (Minor #2).
  - `ui/rgb.rs` & `ui/hardware.rs`: route all writes through `HidWorkerHandle`; wrap unavailable controls in `ui.add_enabled(can_control_via, ...)` and Ember-only controls in `ui.add_enabled(can_control_ember, ...)`.
  - `ui/macros.rs` & `ui/tester.rs`: **remove `cur_mods.command -> 0xE3` completely** (Critical #6); poll `win_sys::poll_win32_special_keys()` each frame for accurate `LCtrl`/`RCtrl`/`LShift`/`RShift`/`LAlt`/`RAlt`/`LWin`/`RWin`/`CapsLock`/`PrtSc`/`ScrLk`/`Pause`/`Menu`/`F13` in both Macro Recorder and Key Tester (Critical #6 + Minor #7); call `validate_macro_text` immediately when clicking Add Text (Important #7).
  - `ui/profiles.rs`: use `storage::sanitize_profile_name` + `storage::resolve_profile_path`; surface every `std::fs::read`/`write`/`remove_file` & `serde_json` error to Toast; pass `Some(self.edition)` to `ApplySnapshot` and `ExportSnapshot`.
  - `ui/autoswitch.rs`: surface `autoswitch::save_config` errors to Toast; validate profile name via `storage::sanitize_profile_name`.
  - `ui/ota.rs`: fix Bluetooth label (`PID_BLUETOOTH`, Minor #1); implement pre-flash confirmation modal displaying `Current CRC -> Target CRC & Name`, hard-lock when `detected_edition == Some(LiteNoRgb)`, and explicit high-risk acknowledgement checkbox when `!is_known` or `!is_known_pro_rgb_source_crc(current_crc)`, passing `OtaFlashPolicy` to `ota::flash_firmware_async`.
  - `main.rs`: use `storage::resolve_data_dir()` (removes hardcoded `d:\Workspace\...`), resolve Windows fonts via `std::env::var("WINDIR")` fallback `C:\Windows` (Minor #6), pass `Arc<Crush80Hid>` to `TrayState::new`, pump `HidWorkerHandle::rx` each frame.

- [ ] **Step 3: Compile & run all Rust tests + clippy**
  Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo build --release`
  Expected: 0 warnings, 0 errors, all unit tests PASS.

- [ ] **Step 4: Commit**
  `git commit -m "refactor(ui): modularize into src/ui/*, wire HidWorker, fix Ctrl+Win macro bug and OTA/error UX"`

---

### Task 7: Ember Firmware `0xFD` GET/SET Protection & Expanded `rc2emu` Verification

**Covers:** Important #6, Minor #9.

**Files:**
- Modify: `build_pro_firmware.py`
- Modify: `crush80-studio-rs/src/ota.rs` (add newly built `Ember v1.3.0` CRC & SHA-256 to known table)
- Modify: `crush80-studio-rs/src/ui/ota.rs` (default search candidate includes `Crush80-RGB-Ember-v1.3.0.bin` first, falling back to `v1.2.0`)

- [ ] **Step 1: Add failing `rc2emu` assertions in `build_pro_firmware.py`**
  In `verify_in_emulator(patched)`:
  1. Test `GET 0xFD` (`[0x08, 0x03, 0xFD, 1, 2, 3, 4, 5]`) does NOT modify `gp[759..763]`, while `SET 0xFD` (`[0x07, 0x03, 0xFD, 200, 100, 50, 3, 0]`) writes `[200, 100, 50, 3, 0]` to `gp[759..763]`.
  2. Test `CONFIG[34]` (Logo color index) and `CONFIG[38]` (Side color index) SET & GET roundtrip.

- [ ] **Step 2: Run `build_pro_firmware.py` to confirm `GET 0xFD` test fails**
  Run: `uv run python build_pro_firmware.py`
  Expected: FAIL on `GET 0xFD` mutation assertion.

- [ ] **Step 3: Apply zero-byte `0xFD` fix in `build_pro_firmware.py` & sync `ota.rs`**
  - Change line 202 jump target from `COPY_HELPER` (`0x1C2FA`) to `CONFIG_HANDLER + 2` (`0x1C2F2`), where `c.andi a4, 1; c.bnez a4, +6` routes `SET` to `COPY_HELPER` (`a1=5`) and `GET` to `CONFIG_GET` (read-only `gp[759]`). Zero extra bytes used (`assert len(out) == 94`).
  - Update `OUT_BIN = ROOT / "Crush80-RGB-Ember-v1.3.0.bin"` and update docstring comments to include `CONFIG[34]/[38]`.
  - Run `uv run python build_pro_firmware.py`, record the exact verified CRC & SHA-256 of `Crush80-RGB-Ember-v1.3.0.bin`, and register it in `crush80-studio-rs/src/ota.rs`.
  - **Do NOT flash the user's connected keyboard automatically.**

- [ ] **Step 4: Re-run `build_pro_firmware.py` and `cargo test`**
  Run: `uv run python build_pro_firmware.py && cargo test`
  Expected: `rc2emu` PASS (including `coldboot`, `0x0E`, `0x12`, Tap-Hold, `SET/GET 0xFD`, `CONFIG[34]/[38]`) and `cargo test` PASS.

- [ ] **Step 5: Commit**
  `git commit -m "fix(firmware): guard VIA 0xFD against GET mutation (0-byte patch) and expand rc2emu tests"`

---

### Task 8: Python Tooling & CI/Release Pipeline Hardening

**Covers:** Important #10, Minor #8, Minor #9.

**Files:**
- Modify: `embed_icon.py`
- Modify: `via_backup.py`
- Modify: `.github/workflows/ci-release.yml`
- Modify: `crush80-studio-rs/Cargo.toml` (bump to `1.3.0`)

- [ ] **Step 1: Harden `embed_icon.py`**
  - Replace `ctypes.windll.kernel32` with `ctypes.WinDLL("kernel32", use_last_error=True)` and replace `ctypes.GetLastError()` with `ctypes.get_last_error()`.
  - If `BeginUpdateResourceW` fails all 3 attempts, raise `OSError(f"BeginUpdateResourceW failed after 3 attempts: {last_err}")` instead of silently returning.
  - Remove unused `ImageFilter` import and unused `prepare_master_rgba`.

- [ ] **Step 2: Harden `via_backup.py`**
  - Prefer wired USB `PID 0x5055` over 2.4G `0x5088` when enumerating `0xFF60`.
  - Check `written = self.dev.write(pkt); if written <= 0: raise OSError(...)`, verify `len(resp) >= 32` (or `>= 4 + size`) and `resp[0] == payload[0]`.
  - Replace `assert len(keymap) == KEYMAP_BYTES` / `assert len(macros) == MACRO_BYTES` with explicit `if ...: raise ValueError(...)`.
  - Backup & restore `rgb["rainbow"]` (`CONFIG[5]` with `0xAA` sentinel) and optional `hw` block on Ember firmware.

- [ ] **Step 3: Harden `.github/workflows/ci-release.yml`**
  - Set top-level `permissions: contents: read`.
  - Split into `build-and-test` job (runs `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, `python -m py_compile build_pro_firmware.py embed_icon.py via_backup.py flash_win.py`, `embed_icon.py`, generates `SHA256SUMS.txt`) and `release` job (`needs: build-and-test`, `if: startsWith(github.ref, 'refs/tags/v')`, `permissions: contents: write`) uploading `Crush80_Studio.exe` + `SHA256SUMS.txt`.
  - Pin `pillow==11.2.1` and pin GitHub Actions to verified full commit SHAs with version comments.

- [ ] **Step 4: Verify Python scripts & Rust release build**
  Run: `python -m py_compile build_pro_firmware.py embed_icon.py via_backup.py flash_win.py` + `cargo test` + `cargo clippy --all-targets -- -D warnings` + `cargo build --release` + `uv run --with pillow python embed_icon.py`.
  Expected: All pass cleanly with zero warnings.

- [ ] **Step 5: Commit**
  `git commit -m "chore(ci,tools): harden workflow permissions/tests/checksums, embed_icon.py and via_backup.py"`

---

### Task 9: Final End-to-End Verification & Independent Read-Only Review

- [ ] **Step 1: Full verification suite**
  - Run `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, `uv run --with pillow python embed_icon.py`, `uv run python build_pro_firmware.py`.
  - Deploy `Crush80_Studio.exe` (`v1.3.0`) and launch a live smoke test verifying clean startup, zero crashes, and portable `studio/` data compatibility.

- [ ] **Step 2: Dispatch fresh Read-Only Senior Code Reviewer subagent**
  - Review full diff `affe9826..HEAD` against the original 28-item audit checklist to confirm all Critical, Important, and Minor items are closed with zero new regressions.

- [ ] **Step 3: Report results & await user go-ahead before pushing/tagging `v1.3.0`**
