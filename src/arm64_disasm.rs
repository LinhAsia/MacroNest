//! Minimalist AArch64 (ARM64) instruction decoder for live Android `libil2cpp.so` inspection.

pub fn decode_arm64_block(bytes: &[u8], base_va: usize) -> Vec<(usize, String, String)> {
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for (idx, chunk) in bytes.chunks_exact(4).enumerate() {
        let va = base_va + idx * 4;
        let insn = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let hex_bytes = format!("{:02X} {:02X} {:02X} {:02X}", chunk[0], chunk[1], chunk[2], chunk[3]);
        out.push((va, hex_bytes, decode_arm64_insn(insn, va)));
    }
    out
}

fn reg_x(r: u32, use_sp: bool) -> &'static str {
    const X: [&str; 32] = [
        "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7",
        "x8", "x9", "x10", "x11", "x12", "x13", "x14", "x15",
        "x16", "x17", "x18", "x19", "x20", "x21", "x22", "x23",
        "x24", "x25", "x26", "x27", "x28", "x29", "x30", "xzr",
    ];
    let idx = (r & 31) as usize;
    if idx == 31 && use_sp { "sp" } else { X[idx] }
}

fn reg_w(r: u32, use_sp: bool) -> &'static str {
    const W: [&str; 32] = [
        "w0", "w1", "w2", "w3", "w4", "w5", "w6", "w7",
        "w8", "w9", "w10", "w11", "w12", "w13", "w14", "w15",
        "w16", "w17", "w18", "w19", "w20", "w21", "w22", "w23",
        "w24", "w25", "w26", "w27", "w28", "w29", "w30", "wzr",
    ];
    let idx = (r & 31) as usize;
    if idx == 31 && use_sp { "wsp" } else { W[idx] }
}

fn reg(r: u32, is_64: bool, use_sp: bool) -> &'static str {
    if is_64 { reg_x(r, use_sp) } else { reg_w(r, use_sp) }
}

fn sign_extend(val: u32, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((val as i64) << shift) >> shift
}

fn cond_name(c: u32) -> &'static str {
    const C: [&str; 16] = [
        "eq", "ne", "cs", "cc", "mi", "pl", "vs", "vc",
        "hi", "ls", "ge", "lt", "gt", "le", "al", "nv",
    ];
    C[(c & 15) as usize]
}

pub fn decode_arm64_insn(insn: u32, pc: usize) -> String {
    if insn == 0xD503_201F {
        return "nop".to_owned();
    }
    if insn == 0xD65F_03C0 {
        return "ret".to_owned();
    }
    if (insn & 0xFFFF_FC1F) == 0xD65F_0000 {
        return format!("ret {}", reg_x((insn >> 5) & 31, false));
    }
    if (insn & 0xFFFF_FC1F) == 0xD61F_0000 {
        return format!("br {}", reg_x((insn >> 5) & 31, false));
    }
    if (insn & 0xFFFF_FC1F) == 0xD63F_0000 {
        return format!("blr {}", reg_x((insn >> 5) & 31, false));
    }

    // B / BL imm26
    if (insn & 0x7C00_0000) == 0x1400_0000 {
        let is_bl = (insn >> 31) != 0;
        let off = sign_extend(insn & 0x03FF_FFFF, 26) * 4;
        let target = (pc as i64).wrapping_add(off) as usize;
        return format!("{} 0x{:X}", if is_bl { "bl" } else { "b" }, target);
    }

    // B.cond imm19
    if (insn & 0xFF00_0010) == 0x5400_0000 {
        let off = sign_extend((insn >> 5) & 0x7FFFF, 19) * 4;
        let target = (pc as i64).wrapping_add(off) as usize;
        return format!("b.{} 0x{:X}", cond_name(insn & 15), target);
    }

    // CBZ / CBNZ
    if (insn & 0x7E00_0000) == 0x3400_0000 {
        let is_64 = (insn >> 31) != 0;
        let is_nz = ((insn >> 24) & 1) != 0;
        let rt = reg(insn & 31, is_64, false);
        let off = sign_extend((insn >> 5) & 0x7FFFF, 19) * 4;
        let target = (pc as i64).wrapping_add(off) as usize;
        return format!("{} {}, 0x{:X}", if is_nz { "cbnz" } else { "cbz" }, rt, target);
    }

    // TBZ / TBNZ
    if (insn & 0x7E00_0000) == 0x3600_0000 {
        let is_nz = ((insn >> 24) & 1) != 0;
        let bit = ((insn >> 31) << 5) | ((insn >> 19) & 31);
        let rt = reg(insn & 31, (insn >> 31) != 0, false);
        let off = sign_extend((insn >> 5) & 0x3FFF, 14) * 4;
        let target = (pc as i64).wrapping_add(off) as usize;
        return format!("{} {}, #{}, 0x{:X}", if is_nz { "tbnz" } else { "tbz" }, rt, bit, target);
    }

    // ADR / ADRP
    if (insn & 0x1F00_0000) == 0x1000_0000 {
        let is_adrp = (insn >> 31) != 0;
        let immlo = (insn >> 29) & 3;
        let immhi = (insn >> 5) & 0x7FFFF;
        let imm = sign_extend((immhi << 2) | immlo, 21);
        let rd = reg_x(insn & 31, false);
        if is_adrp {
            let page = ((pc as i64) & !0xFFF).wrapping_add(imm << 12) as usize;
            return format!("adrp {}, 0x{:X}", rd, page);
        } else {
            let target = (pc as i64).wrapping_add(imm) as usize;
            return format!("adr {}, 0x{:X}", rd, target);
        }
    }

    // ADD / ADDS / SUB / SUBS (immediate)
    if (insn & 0x1F80_0000) == 0x1100_0000 {
        let is_64 = (insn >> 31) != 0;
        let is_sub = ((insn >> 30) & 1) != 0;
        let sets_flags = ((insn >> 29) & 1) != 0;
        let sh = ((insn >> 22) & 1) != 0;
        let imm12 = ((insn >> 10) & 0xFFF) << if sh { 12 } else { 0 };
        let rn = (insn >> 5) & 31;
        let rd = insn & 31;
        if is_sub && sets_flags && rd == 31 {
            return format!("cmp {}, #0x{:X}", reg(rn, is_64, true), imm12);
        }
        if !is_sub && !sets_flags && imm12 == 0 && (rd == 31 || rn == 31) {
            return format!("mov {}, {}", reg(rd, is_64, true), reg(rn, is_64, true));
        }
        let op = match (is_sub, sets_flags) {
            (false, false) => "add",
            (false, true) => "adds",
            (true, false) => "sub",
            (true, true) => "subs",
        };
        return format!("{} {}, {}, #0x{:X}", op, reg(rd, is_64, !sets_flags), reg(rn, is_64, true), imm12);
    }

    // MOVZ / MOVN / MOVK
    if (insn & 0x1F80_0000) == 0x1280_0000 {
        let is_64 = (insn >> 31) != 0;
        let opc = (insn >> 29) & 3;
        let hw = ((insn >> 21) & 3) * 16;
        let imm16 = (insn >> 5) & 0xFFFF;
        let rd = reg(insn & 31, is_64, false);
        let op = match opc {
            0 => "movn",
            2 => "movz",
            3 => "movk",
            _ => return format!(".inst 0x{:08X}", insn),
        };
        if hw == 0 {
            return format!("{} {}, #0x{:X}", op, rd, imm16);
        }
        return format!("{} {}, #0x{:X}, lsl #{}", op, rd, imm16, hw);
    }

    // ORR (shifted register) -> MOV reg, reg when rn == xzr/wzr
    if (insn & 0x7FE0_FFE0) == 0x2A00_03E0 {
        let is_64 = (insn >> 31) != 0;
        let rm = (insn >> 16) & 31;
        let rd = insn & 31;
        return format!("mov {}, {}", reg(rd, is_64, false), reg(rm, is_64, false));
    }

    // STP / LDP (64-bit / 32-bit GPR)
    if (insn & 0x3E00_0000) == 0x2800_0000 && ((insn >> 26) & 1) == 0 {
        let opc = (insn >> 30) & 3;
        let is_load = ((insn >> 22) & 1) != 0;
        let mode = (insn >> 23) & 3;
        if opc == 0 || opc == 2 {
            let is_64 = opc == 2;
            let scale = if is_64 { 8 } else { 4 };
            let imm7 = sign_extend((insn >> 15) & 0x7F, 7) * scale;
            let rt2 = reg((insn >> 10) & 31, is_64, false);
            let rn = reg_x((insn >> 5) & 31, true);
            let rt = reg(insn & 31, is_64, false);
            let op = if is_load { "ldp" } else { "stp" };
            return match mode {
                1 => format!("{op} {rt}, {rt2}, [{rn}], #{imm7}"),
                2 => format!("{op} {rt}, {rt2}, [{rn}, #{imm7}]"),
                3 => format!("{op} {rt}, {rt2}, [{rn}, #{imm7}]!"),
                _ => format!(".inst 0x{:08X}", insn),
            };
        }
    }

    // LDR / STR unsigned immediate (64-bit / 32-bit GPR)
    if (insn & 0x3B20_0000) == 0x3900_0000 {
        let size = (insn >> 30) & 3;
        let is_simd = ((insn >> 26) & 1) != 0;
        let opc = (insn >> 22) & 3;
        if !is_simd && size >= 2 && opc <= 1 {
            let is_64 = size == 3;
            let imm12 = ((insn >> 10) & 0xFFF) << size;
            let rn = reg_x((insn >> 5) & 31, true);
            let rt = reg(insn & 31, is_64, false);
            let op = if opc == 1 { "ldr" } else { "str" };
            if imm12 == 0 {
                return format!("{op} {rt}, [{rn}]");
            }
            return format!("{op} {rt}, [{rn}, #0x{imm12:X}]");
        }
    }

    format!(".inst 0x{:08X}", insn)
}

#[cfg(test)]
mod tests {
    use super::decode_arm64_insn;

    #[test]
    fn decodes_common_arm64_instructions() {
        assert_eq!(decode_arm64_insn(0xD503_201F, 0x1000), "nop");
        assert_eq!(decode_arm64_insn(0xD65F_03C0, 0x1000), "ret");
        assert_eq!(decode_arm64_insn(0xA9BF_7BFD, 0x1000), "stp x29, x30, [sp, #-16]!");
        assert_eq!(decode_arm64_insn(0xA8C1_7BFD, 0x1000), "ldp x29, x30, [sp], #16");
        assert_eq!(decode_arm64_insn(0x9100_03FD, 0x1000), "mov x29, sp");
        assert_eq!(decode_arm64_insn(0x9400_0010, 0x1000), "bl 0x1040");
        assert_eq!(decode_arm64_insn(0xD280_0020, 0x1000), "movz x0, #0x1");
        assert_eq!(decode_arm64_insn(0xF940_0800, 0x1000), "ldr x0, [x0, #0x10]");
        assert_eq!(decode_arm64_insn(0xAA1F_03E1, 0x1000), "mov x1, xzr");
    }
}
