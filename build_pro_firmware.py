#!/usr/bin/env python3
"""Build & emulate-verify Crush 80 Ember firmware (based on NativeHeld).

Features added on top of verified NativeHeld v1.3.0 (all 100% rc2emu verified):
  1. Universal VIA CONFIG[5..62] Read/Write (channel 3, id = 5..62):
     - id 5  : Main RGB Rainbow/Single (0xFF = Rainbow, 0x00 = Single)
     - id 21 : Win Key Lock (0xFF = Locked, 0x00 = Unlocked)
     - id 26 : OS Mode (0x00 = Windows L0/1, 0x01 = macOS L2/3)
     - id 29 : Ultra-Low-Latency ISR Debounce (0xFF = ISR Fast, 0x00 = Standard)
     - id 32/33/35 : Logo Badge RGB Mode / Color / Power (0=On, 1=Off)
     - id 36/37/39 : Side Underglow RGB Mode / Color / Power (0=On, 1=Off)
     - id 40 : Master RGB System Power (0=On, 1=Off)
     - id 60 : Dynamic Tap-Hold Tapping Term in 10ms units (0 = default 200ms, e.g. 15 = 150ms)
  2. Dynamic Tap-Hold Tapping Term (100ms..300ms live-adjustable via CONFIG[60]).

OEM bootloader, OTA stack, RGB startup, and sleep lifecycle hooks remain 100% untouched.
"""
import hashlib
import struct
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO = ROOT / "wobkey-crush80-reboot-pro-firmware"
SRC_BIN = REPO / "firmware/patched/Crush80-RGB-0108-NativeHeld-20261101.bin"
OUT_BIN = ROOT / "Crush80-RGB-Ember.bin"

EXPECTED_SRC_SHA256 = "3b6c6755b1a3b263d6e55c2a4415d7e91689ac43f897b3be970b236ef970de3a"


# --- RV32IMC Encoders ---
def enc_lui(rd, imm20):
    return ((imm20 & 0xFFFFF) << 12) | (rd << 7) | 0x37

def enc_addi(rd, rs1, imm12):
    return ((imm12 & 0xFFF) << 20) | (rs1 << 15) | (0 << 12) | (rd << 7) | 0x13

def enc_sltiu(rd, rs1, imm12):
    return ((imm12 & 0xFFF) << 20) | (rs1 << 15) | (3 << 12) | (rd << 7) | 0x13

def enc_xori(rd, rs1, imm12):
    return ((imm12 & 0xFFF) << 20) | (rs1 << 15) | (4 << 12) | (rd << 7) | 0x13

def enc_mul(rd, rs1, rs2):
    return (1 << 25) | (rs2 << 20) | (rs1 << 15) | (0 << 12) | (rd << 7) | 0x33

def enc_lbu(rd, rs1, imm12):
    return ((imm12 & 0xFFF) << 20) | (rs1 << 15) | (4 << 12) | (rd << 7) | 0x03

def enc_sb(rs2, rs1, imm12):
    imm = imm12 & 0xFFF
    return ((imm >> 5) << 25) | (rs2 << 20) | (rs1 << 15) | (0 << 12) | ((imm & 0x1F) << 7) | 0x23

def enc_bltu(rs1, rs2, offset):
    assert -4096 <= offset <= 4094 and (offset & 1) == 0
    imm = offset & 0x1FFF
    return (((imm >> 12) & 1) << 31) | (((imm >> 5) & 0x3F) << 25) | (rs2 << 20) | (rs1 << 15) | (6 << 12) | (((imm >> 1) & 0xF) << 8) | (((imm >> 11) & 1) << 7) | 0x63

def enc_jal(rd, offset):
    assert -(1 << 20) <= offset < (1 << 20) and (offset & 1) == 0
    imm = offset & 0x1FFFFF
    return (((imm >> 20) & 1) << 31) | (((imm >> 1) & 0x3FF) << 21) | (((imm >> 11) & 1) << 20) | (((imm >> 12) & 0xFF) << 12) | (rd << 7) | 0x6F

def enc_c_li(rd, imm6):
    imm = imm6 & 0x3F
    return (0b010 << 13) | (((imm >> 5) & 1) << 12) | (rd << 7) | ((imm & 0x1F) << 2) | 0b01

def enc_c_mv(rd, rs2):
    return (0b100 << 13) | (0 << 12) | (rd << 7) | (rs2 << 2) | 0b10

def enc_c_add(rd, rs2):
    return (0b100 << 13) | (1 << 12) | (rd << 7) | (rs2 << 2) | 0b10

def enc_c_jr(rs1):
    return (0b100 << 13) | (0 << 12) | (rs1 << 7) | (0 << 2) | 0b10

def enc_c_slli(rd, shamt):
    return (0b000 << 13) | (((shamt >> 5) & 1) << 12) | (rd << 7) | ((shamt & 0x1F) << 2) | 0b10

def enc_c_srli(rd_p, shamt):
    assert 8 <= rd_p <= 15
    return (0b100 << 13) | (((shamt >> 5) & 1) << 12) | (0b00 << 10) | ((rd_p - 8) << 7) | ((shamt & 0x1F) << 2) | 0b01

def enc_c_andi(rd_p, imm6):
    assert 8 <= rd_p <= 15
    imm = imm6 & 0x3F
    return (0b100 << 13) | (((imm >> 5) & 1) << 12) | (0b10 << 10) | ((rd_p - 8) << 7) | ((imm & 0x1F) << 2) | 0b01

def enc_c_beqz(rs1_p, offset):
    assert 8 <= rs1_p <= 15 and -256 <= offset <= 254 and (offset & 1) == 0
    imm = offset & 0x1FF
    return (0b110 << 13) | (((imm >> 8) & 1) << 12) | (((imm >> 3) & 3) << 10) | ((rs1_p - 8) << 7) | (((imm >> 6) & 3) << 5) | (((imm >> 1) & 3) << 3) | (((imm >> 5) & 1) << 2) | 0b01

def enc_c_bnez(rs1_p, offset):
    assert 8 <= rs1_p <= 15 and -256 <= offset <= 254 and (offset & 1) == 0
    imm = offset & 0x1FF
    return (0b111 << 13) | (((imm >> 8) & 1) << 12) | (((imm >> 3) & 3) << 10) | ((rs1_p - 8) << 7) | (((imm >> 6) & 3) << 5) | (((imm >> 1) & 3) << 3) | (((imm >> 5) & 1) << 2) | 0b01

def enc_c_nop():
    return 0x0001


ZERO, RA, SP, GP = 0, 1, 2, 3
T0, T1 = 5, 6
A0, A1, A2, A3, A4, A5 = 10, 11, 12, 13, 14, 15
T5, T6 = 30, 31


def build_patch(src: bytes) -> bytes:
    assert hashlib.sha256(src).hexdigest() == EXPECTED_SRC_SHA256, "Source binary SHA-256 mismatch!"
    img = bytearray(src)

    # =========================================================================
    # PATCH 1: Universal VIA CONFIG[5..62] Read/Write (channel 3, id = 5..62)
    # =========================================================================
    # 1A. Compacted id=4 + dispatch trampoline at 0x12E6C..0x12ECA (94B budget)
    out = bytearray()
    def emit32(b, w): b.extend(struct.pack("<I", w))
    def emit16(b, h): b.extend(struct.pack("<H", h))

    # id=4: SET Single Color using gp directly
    emit32(out, enc_sb(ZERO, GP, 109))       # CONFIG[5] = 0
    emit32(out, enc_sb(A4, GP, 113))         # CONFIG[9] = hue
    emit16(out, enc_c_mv(A3, A4))
    emit16(out, enc_c_slli(A3, 1))
    emit16(out, enc_c_add(A4, A3))
    emit32(out, enc_lui(A3, 0x2001C))
    emit32(out, enc_addi(A3, A3, 0x65C))
    emit16(out, enc_c_add(A4, A3))
    emit32(out, enc_lbu(A3, A4, 0))
    emit32(out, enc_sb(A3, GP, 110))
    emit32(out, enc_lbu(A3, A4, 1))
    emit32(out, enc_sb(A3, GP, 111))
    emit32(out, enc_lbu(A3, A4, 2))
    emit32(out, enc_sb(A3, GP, 112))
    emit32(out, enc_lbu(A5, A5, 4))
    emit32(out, enc_xori(A5, A5, -1))
    emit32(out, enc_sb(A5, GP, 134))
    shared_ret_off = len(out)                # +0x3C (0x12EA8)
    emit16(out, enc_c_jr(RA))

    # id >= 5 entry at 0x12EAA:
    #   - if packet[2] == 0xFE (a0 == 0xFD): read live battery % from gp + 0x10C (0x8090C) -> packet[3]
    #   - else if packet[2] in 5..62 (a0 < 62): set a2 = gp + 105, jump to 0x1C2F0
    id5_off = len(out)                       # +0x3E (0x12EAA)
    emit32(out, enc_addi(A1, ZERO, 0xFD))    # +0x3E (4B): a1 = 0xFD (for packet[2] == 0xFE)
    # bne a0, a1, +14 -> skip battery read (4+4+2 = 10B -> +14 from +0x42!)
    # Encode bne a0, a1, +14: funct3 = 1
    def enc_bne(rs1, rs2, offset):
        imm = offset & 0x1FFF
        return (((imm >> 12) & 1) << 31) | (((imm >> 5) & 0x3F) << 25) | (rs2 << 20) | (rs1 << 15) | (1 << 12) | (((imm >> 1) & 0xF) << 8) | (((imm >> 11) & 1) << 7) | 0x63
    emit32(out, enc_bne(A0, A1, 14))         # +0x42 (4B)
    emit32(out, enc_lbu(A3, GP, 0x10C))      # +0x46 (4B): a3 = true battery % (gp+0x10C = 0x8090C, 0..100)
    emit32(out, enc_sb(A3, A5, 3))           # +0x4A (4B): packet[3] = battery %
    emit16(out, enc_c_jr(RA))                # +0x4E (2B): return!
    assert len(out) == 0x50
    emit32(out, enc_sltiu(A1, A0, 62))       # +0x50 (4B): a1 = (a0 < 62) -> packet[2] in 5..62
    emit16(out, enc_c_beqz(A1, shared_ret_off - len(out)))  # +0x54 (2B): if out of range -> ret
    emit32(out, enc_addi(A2, GP, 105))       # +0x56 (4B): a2 = &CONFIG[1]
    jmp_pc = 0x12E6C + len(out)              # 0x12EC6
    emit32(out, enc_jal(ZERO, 0x1C2F0 - jmp_pc))  # +0x5A (4B): jump to 0x1C2F0
    assert len(out) == 94, f"Expected exact 94B, got {len(out)}B"

    # Redirect 0x12DB0 bltu a1, a0 -> 0x12EAA
    orig_br = struct.unpack_from("<I", img, 0x12DB0)[0]
    assert orig_br == enc_bltu(A1, A0, 0x1C4)
    struct.pack_into("<I", img, 0x12DB0, enc_bltu(A1, A0, (0x12E6C + id5_off) - 0x12DB0))
    img[0x12E6C : 0x12E6C + 94] = out

    # 1B. Universal CONFIG[id] GET/SET handler at free reclaimed gap 0x1C2F0..0x1C30C (28B budget)
    stub = bytearray()
    emit16(stub, enc_c_add(A2, A0))          # +0x00 (2B): a2 = &CONFIG[packet[2]]
    emit16(stub, enc_c_andi(A4, 1))          # +0x02 (2B): 1 if SET(7), 0 if GET(8)
    emit16(stub, enc_c_bnez(A4, 12))         # +0x04 (2B): if SET -> +0x10
    # GET (cmd 8): packet[3] = CONFIG[id]
    emit32(stub, enc_lbu(A3, A2, 0))         # +0x06 (4B)
    emit32(stub, enc_sb(A3, A5, 3))          # +0x0A (4B)
    emit16(stub, enc_c_jr(RA))               # +0x0E (2B)
    # SET (cmd 7): CONFIG[id] = packet[3]
    assert len(stub) == 0x10
    emit32(stub, enc_lbu(A3, A5, 3))         # +0x10 (4B)
    emit32(stub, enc_sb(A3, A2, 0))          # +0x14 (4B)
    emit16(stub, enc_c_jr(RA))               # +0x18 (2B)
    assert len(stub) == 26 <= 28
    img[0x1C2F0 : 0x1C2F0 + len(stub)] = stub

    # =========================================================================
    # PATCH 2: Dynamic Tap-Hold Tapping Term via CONFIG[60] (10ms units)
    # =========================================================================
    # Helper at free reclaimed gap 0x0E748..0x0E75A (18B exact!)
    # Uses T6 (x31) as link reg and T5 (x30) as temp multiplier so RA & A0..A5 are untouched!
    tt = bytearray()
    emit32(tt, enc_lbu(A5, GP, 164))         # +0x00 (4B): a5 = CONFIG[60] (10ms units)
    emit16(tt, enc_c_bnez(A5, 4))            # +0x04 (2B): if != 0, skip default
    emit16(tt, enc_c_li(A5, 20))             # +0x06 (2B): default 20 (= 200ms)
    emit32(tt, enc_addi(T5, ZERO, 625))      # +0x08 (4B): 625 ticks = 10ms @ 62.5kHz
    emit32(tt, enc_mul(A5, A5, T5))          # +0x0C (4B): a5 = units * 625
    emit16(tt, enc_c_jr(T6))                 # +0x10 (2B): return via T6
    assert len(tt) == 18
    img[0x0E748 : 0x0E748 + 18] = tt

    # Patch both Tap-Hold deadline sites (0x0DC34 and 0x0DFA4, each 8 bytes: c.lui + c.srli + addi)
    for site in (0x0DC34, 0x0DFA4):
        orig_srli = struct.unpack_from("<H", img, site + 2)[0]
        assert orig_srli == enc_c_srli(A0, 8), f"Unexpected insn at {site+2:#x}"
        call_seq = bytearray()
        emit16(call_seq, enc_c_srli(A0, 8))                  # +0 (2B): preserve a0 >>= 8
        emit32(call_seq, enc_jal(T6, 0x0E748 - (site + 2)))  # +2 (4B): call helper -> a5 = ticks
        emit16(call_seq, enc_c_nop())                        # +6 (2B): pad to 8B
        assert len(call_seq) == 8
        img[site : site + 8] = call_seq

    # Recompute Telink trailing CRC32
    crc = (~zlib.crc32(img[:-4])) & 0xFFFFFFFF
    struct.pack_into("<I", img, len(img) - 4, crc)
    assert zlib.crc32(img) == 0xFFFFFFFF
    return bytes(img)


def verify_in_emulator(patched: bytes):
    sys.path.insert(0, str(REPO / "tests"))
    from rc2emu.keyboard import Keyboard
    from rc2emu.machine import Machine, TICKS_PER_STEP

    m = Machine(patched)
    PKT = 0x80F00
    VIA = 0x20012C28

    def via(pkt: bytes) -> bytes:
        m.bus.poke(PKT, pkt + bytes(32 - len(pkt)))
        m.call(VIA, PKT)
        return m.bus.peek(PKT, 8)

    # 1. Test universal CONFIG[5..62] GET/SET across all key registers
    for reg_id, test_val in [
        (5, 0xFF),   # Main Rainbow ON
        (21, 0xFF),  # WinLock ON
        (26, 0x01),  # macOS mode
        (29, 0xFF),  # Low-Latency ISR Debounce ON
        (32, 0x02),  # Logo Mode
        (33, 0xFF),  # Logo Rainbow
        (35, 0x01),  # Logo Power OFF
        (36, 0x03),  # Side Mode
        (37, 0x00),  # Side Color
        (39, 0x01),  # Side Power OFF
        (40, 0x01),  # Master RGB Power OFF
        (60, 14),    # Tap-Hold Tapping Term = 140ms
    ]:
        via(bytes([0x07, 0x03, reg_id, test_val]))
        assert m.bus.peek(0x80868 + reg_id, 1)[0] == test_val, f"SET CONFIG[{reg_id}] failed"
        got = via(bytes([0x08, 0x03, reg_id]))[3]
        assert got == test_val, f"GET CONFIG[{reg_id}] mismatch: {got} != {test_val}"

    # 2. Test battery % read (id=0xFE -> gp+0x10C = 0x8090C)
    m.bus.poke(0x8090C, bytes([87]))
    bat = via(bytes([0x08, 0x03, 0xFE]))[3]
    assert bat == 87, f"Battery read mismatch: {bat} != 87"

    # 3. Test out-of-range ids (0, 63, 64, 255) are safely ignored
    via(bytes([0x07, 0x03, 0, 0xAA]))
    via(bytes([0x07, 0x03, 63, 0xAA]))
    via(bytes([0x07, 0x03, 255, 0xAA]))
    assert m.bus.peek(0x80868 + 63, 1)[0] != 0xAA

    # 3. Test id=4 Single Color still clears CONFIG[5]=0 and sets palette RGB properly
    via(bytes([0x07, 0x03, 4, 135, 220]))
    cfg = m.bus.peek(0x80868, 32)
    assert cfg[5] == 0 and cfg[9] == 135 and cfg[30] == (220 ^ 0xFF)

    # 4. Test Dynamic Tap-Hold Tapping Term on real MT(LCtrl, Esc) = 0x2129 in rc2emu!
    km = [[0] * 128 for _ in range(4)]
    km[0][48] = 0x2129  # CapsLock (R3,C0 = pos 48) = MT(LCtrl, KC_ESC)
    km[0][49] = 0x0004  # A (R3,C1 = pos 49) = KC_A
    for term_units, hold_ms, expect_ctrl in [
        (0, 130, False),   # Default 200ms: 130ms hold -> Tap (Esc)
        (10, 130, True),   # Custom 100ms:  130ms hold -> Hold (LCtrl + A!)
        (28, 220, False),  # Custom 280ms:  220ms hold -> Tap (Esc)
    ]:
        kb = Keyboard(km, image=patched, fast_delay=True)
        kb.machine.bus.poke(0x80868 + 60, bytes([term_units]))
        kb.key(48, True)
        # Advance 16MHz timer by exact hold_ms
        kb.machine.cpu.total += int(hold_ms * 16000) // TICKS_PER_STEP
        if expect_ctrl:
            kb.key(49, True)   # Press A while holding CapsLock past tapping term -> must emit LCtrl + A!
            kb.key(49, False)
        kb.key(48, False)
        kb.idle(2)
        reports = [r[1] for r in kb.take() if r[0] == 1]
        saw_ctrl_a = any(r.startswith("01") and "04" in r[4:16] for r in reports)
        saw_esc = any("29" in r[4:16] for r in reports)
        if expect_ctrl:
            assert saw_ctrl_a, f"Expected LCtrl+A hold for term={term_units*10}ms, hold={hold_ms}ms; reports={reports}"
        else:
            assert saw_esc and not saw_ctrl_a, f"Expected Esc tap for term={term_units*10}ms, hold={hold_ms}ms; reports={reports}"

    print("ALL PRO-SUITE RISC-V EMULATOR VERIFICATION CHECKS PASSED 100%!")


if __name__ == "__main__":
    src = SRC_BIN.read_bytes()
    patched = build_patch(src)
    verify_in_emulator(patched)
    OUT_BIN.write_bytes(patched)
    sha = hashlib.sha256(patched).hexdigest()
    crc = struct.unpack_from("<I", patched, len(patched) - 4)[0]
    print(f"Built   : {OUT_BIN}")
    print(f"Size    : {len(patched)} bytes")
    print(f"SHA-256 : {sha}")
    print(f"OTA CRC : 0x{crc:08X}")
