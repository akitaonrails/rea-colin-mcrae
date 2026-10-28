//! The hi-score tables' verification codes `FUN_004cc260`: each present
//! record is packed into a word `D` and a byte `B` and turned into an
//! all-digit string by `FUN_0042a0b0`/`FUN_0042a1a0` over the executable's
//! key schedule `0x4ed118` (32 bytes, from the user's executable). `D`
//! starts at `0x5a5a5a5a` once per table build and carries the bits a record
//! does not overwrite into the next; every bit of `B` is rewritten per
//! record (its two low bits are masked to a per-table constant). An absent
//! record's code is empty. A description of disassembly, not original source. Evidence:
//! `rea.official-eu.hiscore-codes`.

/// The key schedule's address.
pub const SCHEDULE_BASE: usize = 0x4ed118;
/// Its length.
pub const SCHEDULE_LEN: usize = 32;

fn at(schedule: &[u8], address: usize) -> u8 {
    schedule.get(address - SCHEDULE_BASE).copied().unwrap_or(0)
}

/// `FUN_0042a0b0(key, &D, &B, out)`: mix the schedule into `D`'s bytes and
/// a check byte, then `FUN_0042a1a0`.
pub fn code(key: u32, d: u32, b: u8, schedule: &[u8]) -> Vec<u8> {
    let mut bytes = d.to_le_bytes();
    for (s, byte) in bytes.iter_mut().enumerate() {
        let mix = at(schedule, 0x4ed130 + s)
            .wrapping_add(at(schedule, 0x4ed124 + s))
            .wrapping_add(at(schedule, 0x4ed118 + s))
            .wrapping_add(at(schedule, 0x4ed122 - s).wrapping_mul(3));
        *byte = byte.wrapping_add(mix);
    }
    let check = [0x4ed136, 0x4ed12a, 0x4ed11e, 0x4ed134, 0x4ed128, 0x4ed11c]
        .into_iter()
        .fold(b, |sum, address| sum.wrapping_add(at(schedule, address)));
    digits(key, u32::from_le_bytes(bytes), check)
}

/// `FUN_0042a1a0(key, D, chk, out)`: `D`'s decimal digits least significant
/// first (none for 0), then five digits of `(chk << 8 | sum)`, where `sum`
/// adds `D`'s bytes, `chk` and 2 before the key is applied.
fn digits(key: u32, mut d: u32, mut check: u8) -> Vec<u8> {
    let sum = d
        .to_le_bytes()
        .iter()
        .fold(check.wrapping_add(2), |s, &byte| s.wrapping_add(byte));
    if key as i32 > 0 {
        d ^= key;
        check ^= key as u8;
    }
    let mut out = Vec::new();
    while d != 0 {
        out.push(b'0' + (d % 10) as u8);
        d /= 10;
    }
    let mut tail = (u32::from(check) << 8 | u32::from(sum)) & 0xffff;
    for _ in 0..5 {
        out.push(b'0' + (tail % 10) as u8);
        tail /= 10;
    }
    out
}

fn xor_bytes(d: u32, mask: u8) -> u32 {
    u32::from_le_bytes(d.to_le_bytes().map(|byte| byte ^ mask))
}

/// The best stage times' 56 codes (countries 0..8 × stages 0..7, index
/// `country * 7 + stage`; the record `img + 0xffc + 30 index` has its time
/// at `+0x1a` and its flags at `+0x1c`, bit 4 present).
pub fn best_stage_codes(img: &[u8], schedule: &[u8]) -> Vec<Vec<u8>> {
    let mut d = 0x5a5a_5a5a_u32;
    let mut codes = Vec::with_capacity(56);
    for country in 0..8u32 {
        for stage in 0..7u32 {
            let base = 0x1016 + ((country * 7 + stage) * 30) as usize;
            let flags = img[base + 2];
            if flags & 0x10 == 0 {
                codes.push(Vec::new());
                continue;
            }
            let t = u32::from(u16::from_le_bytes([img[base], img[base + 1]]));
            let w = u32::from(u16::from_le_bytes([img[base + 2], img[base + 3]]));
            d = (d & 0xffff_f03f) | ((stage & 7) << 3 | (country & 7)) << 6;
            d = (d & 0xffff_0fff) | (u32::from(flags) & 0xf) << 12;
            d = (d & 0xdfff_ffff) | (u32::from(flags) & 0x20) << 24;
            d = (d & 0xffff_ffc0) | (t / 6000) & 0x3f;
            d = (d & 0xffc0_ffff) | (((t / 100) % 60) & 0x3f) << 16;
            d = (d & 0xe03f_ffff) | ((t % 100) & 0x7f) << 22;
            let b = ((w >> 6) << 2) as u8;
            d = xor_bytes(d, (((w >> 6) & 0x3f) << 1) as u8);
            codes.push(code(0, d, b, schedule));
        }
    }
    codes
}

/// The best rally times' codes (countries 0..8 × places 0..5, index
/// `country * 7 + place`; the dword `img + 0xd88 + 80 country + 16 place`:
/// time in bits 0-18, car 19-22, present 23, a flag 24, bits 25-31).
pub fn best_rally_codes(img: &[u8], schedule: &[u8]) -> Vec<Option<Vec<u8>>> {
    let mut d = 0x5a5a_5a5a_u32;
    let mut codes = vec![None; 56];
    for country in 0..8u32 {
        for place in 0..5u32 {
            let at = 0xd88 + (country * 80 + place * 16) as usize;
            let r = u32::from_le_bytes([img[at], img[at + 1], img[at + 2], img[at + 3]]);
            let index = (country * 7 + place) as usize;
            if r & 0x80_0000 == 0 {
                codes[index] = Some(Vec::new());
                continue;
            }
            let t = r & 0x7ffff;
            d = (d & 0xffff_fe3f) | (country & 7) << 6;
            d = (d & 0xffff_e1ff) | (r >> 10) & 0x1e00;
            d = (d & 0xffff_dfff) | (r >> 11) & 0x2000;
            d = (d & !0x3f) | (t / 6000) & 0x3f;
            d = (d & 0xfff0_3fff) | (((t / 100) % 60) & 0x3f) << 14;
            d = (d & 0xf80f_ffff) | ((t % 100) & 0x7f) << 20;
            let b = ((r >> 25) << 2) as u8 | 1;
            d = xor_bytes(d, (((r >> 25) & 0x3f) << 1) as u8);
            codes[index] = Some(code(0, d, b, schedule));
        }
    }
    codes
}

/// The championship scores' codes (difficulties 0..3 × places 0..5, index
/// `difficulty * 7 + place`; the record `img + 0xc98 + 75 difficulty + 15
/// place`: `+0xc` bit 7 present and the points, `+0xd` car and a count,
/// `+0xe` flag bits).
pub fn championship_codes(img: &[u8], schedule: &[u8]) -> Vec<Option<Vec<u8>>> {
    let mut d = 0x5a5a_5a5a_u32;
    let mut codes = vec![None; 21];
    for difficulty in 0..3u32 {
        for place in 0..5u32 {
            let base = 0xc98 + (difficulty * 75 + place * 15) as usize;
            let index = (difficulty * 7 + place) as usize;
            let points = img[base + 0xc];
            if points & 0x80 == 0 {
                codes[index] = Some(Vec::new());
                continue;
            }
            let (car, flags) = (u32::from(img[base + 0xd]), img[base + 0xe]);
            d = (d & 0xffff_01ff) | (u32::from(points) & 0x7f) << 9;
            d = (d & 0xffff_fe1f) | (car & 0xf) << 5;
            d = (d & 0xffff_fff0) | car >> 4;
            d = (d & 0xfffc_ffef) | ((difficulty & 3) << 12 | u32::from(flags) & 1) << 4;
            let b = ((flags & 0xfe) << 1) | 2;
            d = xor_bytes(d, flags & 0x7e);
            codes[index] = Some(code(0, d, b, schedule));
        }
    }
    codes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_write_least_significant_first_then_five_check_digits() {
        // D = 1203: "3021", then (0x01 << 8 | sum) where sum = 3+4+0+0+1+2.
        let out = digits(0, 1203, 1);
        assert_eq!(&out[..4], b"3021");
        let tail = (1 << 8) | (0xb3 + 4 + 1 + 2) as u32 & 0xff;
        let expect: Vec<u8> = (0..5)
            .map(|i| b'0' + ((tail / 10u32.pow(i)) % 10) as u8)
            .collect();
        assert_eq!(&out[4..], &expect[..]);
        assert_eq!(digits(0, 0, 0).len(), 5, "no digits for D = 0");
    }

    #[test]
    fn a_zero_schedule_leaves_the_word_and_the_byte() {
        let schedule = [0u8; SCHEDULE_LEN];
        assert_eq!(code(0, 7, 0, &schedule), digits(0, 7, 0));
        // Mixing adds the schedule bytes, three times the mirrored one.
        let mut schedule = [0u8; SCHEDULE_LEN];
        schedule[0x4ed122 - SCHEDULE_BASE] = 1; // mirrors onto byte 0
        assert_eq!(code(0, 0, 0, &schedule), digits(0, 3, 0));
    }

    #[test]
    fn absent_records_are_empty_and_present_ones_digits() {
        let mut img = vec![0u8; 0x1e20];
        let schedule = [0u8; SCHEDULE_LEN];
        let base = 0x1016 + 30 * 8; // country 1, stage 1
        img[base..base + 2].copy_from_slice(&6123u16.to_le_bytes());
        img[base + 2] = 0x13;
        let codes = best_stage_codes(&img, &schedule);
        assert_eq!(codes.len(), 56);
        assert!(codes[0].is_empty());
        assert!(!codes[8].is_empty() && codes[8].iter().all(u8::is_ascii_digit));
        let rally = best_rally_codes(&img, &schedule);
        assert_eq!(rally[5], None, "slots 5 and 6 of a country are unused");
        assert_eq!(rally[0], Some(Vec::new()));
        img[0xc98 + 0xc] = 0x85;
        let champ = championship_codes(&img, &schedule);
        assert!(champ[0].as_ref().is_some_and(|c| !c.is_empty()));
    }
}
