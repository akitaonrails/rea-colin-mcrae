//! The hi-score record writers the results run on the cmRally.cfg record
//! `img`: the best stage time `FUN_004ba370` (one record per stage), the top
//! five rally times `FUN_004ba260` and the top five championship scores
//! `FUN_004ba120` (each inserting and shifting the rest down). A written
//! record also sets `live[0x18] |= 0x40` and counts `live[0x19]`, which the
//! caller applies ([`mark_live`]). Names are copied as `strcpy` does (to the
//! NUL, over whatever follows). A description of disassembly, not original
//! source. Evidence: `rea.official-eu.hiscore-records`.

/// `strcpy(img + at, name)` within the record.
fn copy_name(img: &mut [u8], at: usize, name: &[u8]) {
    let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
    for (i, &b) in name.iter().chain(std::iter::once(&0)).enumerate() {
        if let Some(slot) = img.get_mut(at + i) {
            *slot = b;
        }
    }
}

/// The live block's record flag and count (`live[0x18] |= 0x40`,
/// `live[0x19] += 1`).
pub fn mark_live(live: &mut [u8]) {
    live[0x18] |= 0x40;
    live[0x19] = live[0x19].wrapping_add(1);
}

/// What one stage record holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageRecord<'a> {
    pub name: &'a [u8],
    /// The car (low nibble).
    pub car: u8,
    /// The seven split times.
    pub splits: [u16; 7],
    /// Hundredths.
    pub time: i32,
    /// Bit 5 of the flags (the car byte's bit 4: the manual gearbox).
    pub manual: bool,
    /// Bits 6-11 of the flags (`DAT_0055002c`).
    pub extra: u8,
}

/// `FUN_004ba370(country, stage, ..)`: a new best stage time when `time` is
/// below the stored one (record `img + 0xffc + 30 (7 country + stage)`, or
/// `img + 0x175e + 30 stage` in the secret series `live[0x166] & 0x40`).
pub fn best_stage(
    img: &mut [u8],
    secret: bool,
    country: u8,
    stage: u8,
    record: &StageRecord,
) -> bool {
    let base = if secret {
        0x175e + usize::from(stage) * 30
    } else {
        0xffc + (usize::from(country) * 7 + usize::from(stage)) * 30
    };
    let stored = i32::from(u16::from_le_bytes([img[base + 0x1a], img[base + 0x1b]]));
    if stored <= record.time {
        return false;
    }
    img[base + 0x1a..base + 0x1c].copy_from_slice(&(record.time as u16).to_le_bytes());
    let w = u16::from_le_bytes([img[base + 0x1c], img[base + 0x1d]]);
    let flags = u16::from(record.extra & 0x3f) << 6
        | u16::from(record.manual) << 5
        | (w & 0xf000)
        | 0x10
        | u16::from(record.car & 0xf);
    img[base + 0x1c..base + 0x1e].copy_from_slice(&flags.to_le_bytes());
    copy_name(img, base, record.name);
    for (i, split) in record.splits.iter().enumerate() {
        img[base + 0xc + 2 * i..base + 0xe + 2 * i].copy_from_slice(&split.to_le_bytes());
    }
    true
}

/// `FUN_004ba260(country, name, car, time, manual, extra)`: the place the
/// rally time takes among the country's five (`img + 0xd7c + 80 country`,
/// 16-byte records with the dword `+0xc`: time bits 0-18, car 19-22,
/// present 23, manual 24, extra 25-30, bit 31 kept), or `None` when it beats
/// none. An equal time does not beat a stored one.
#[allow(clippy::too_many_arguments)]
pub fn best_rally(
    img: &mut [u8],
    country: u8,
    name: &[u8],
    car: u8,
    time: i32,
    manual: bool,
    extra: u8,
) -> Option<usize> {
    let base = 0xd7c + usize::from(country) * 0x50;
    let dword = |img: &[u8], place: usize| {
        let at = base + place * 16 + 0xc;
        u32::from_le_bytes([img[at], img[at + 1], img[at + 2], img[at + 3]])
    };
    let place = (0..5).find(|&place| (dword(img, place) & 0x7ffff) as i32 > time)?;
    for k in (place + 1..5).rev() {
        img.copy_within(base + (k - 1) * 16..base + k * 16, base + k * 16);
    }
    let old = dword(img, place);
    let new = u32::from(extra & 0x3f) << 25
        | u32::from(manual) << 24
        | u32::from(car & 0xf) << 19
        | (old & 0x8080_0000)
        | (time as u32 & 0x7ffff)
        | 0x80_0000;
    let at = base + place * 16;
    img[at + 0xc..at + 0x10].copy_from_slice(&new.to_le_bytes());
    copy_name(img, at, name);
    Some(place)
}

/// `FUN_004ba120(name, car, count, points, difficulty, flag, extra)`: the
/// place the championship score takes among the difficulty's five
/// (`img + 0xc98 + 75 difficulty`, 15-byte records: `+0xc` present and the
/// points, `+0xd` car and a count, `+0xe` flag bits), or `None`. More points
/// win; equal points need a higher count.
#[allow(clippy::too_many_arguments)]
pub fn championship(
    img: &mut [u8],
    name: &[u8],
    car: u8,
    count: i32,
    points: i32,
    difficulty: u8,
    flag: bool,
    extra: u8,
) -> Option<usize> {
    let base = 0xc98 + usize::from(difficulty) * 75;
    let place = (0..5).find(|&place| {
        let p = i32::from(img[base + place * 15 + 0xc] & 0x7f);
        let c = i32::from(img[base + place * 15 + 0xd] & 0xf);
        points > p || (points == p && count > c)
    })?;
    for k in (place + 1..5).rev() {
        img.copy_within(base + (k - 1) * 15..base + k * 15, base + k * 15);
    }
    let at = base + place * 15;
    img[at + 0xd] = (car << 4) | (count as u8 & 0xf);
    img[at + 0xc] = (img[at + 0xc] & 0x80) | (points as u8 & 0x7f) | 0x80;
    img[at + 0xe] = ((extra & 0x3f) << 1) | (img[at + 0xe] & 0x80) | u8::from(flag);
    copy_name(img, at, name);
    Some(place)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> Vec<u8> {
        vec![0u8; 0x1e20]
    }

    #[test]
    fn a_stage_record_needs_a_strictly_faster_time() {
        let mut img = img();
        let base = 0xffc + (7 + 2) * 30;
        img[base + 0x1a..base + 0x1c].copy_from_slice(&5000u16.to_le_bytes());
        img[base + 0x1d] = 0xa0; // kept high nibble
        let record = StageRecord {
            name: b"COLIN\0junk",
            car: 0x13,
            splits: [1, 2, 3, 4, 5, 6, 7],
            time: 5000,
            manual: true,
            extra: 0x3f,
        };
        assert!(
            !best_stage(&mut img, false, 1, 2, &record),
            "equal is not a record"
        );
        let faster = StageRecord {
            time: 4999,
            ..record
        };
        assert!(best_stage(&mut img, false, 1, 2, &faster));
        assert_eq!(&img[base..base + 6], b"COLIN\0");
        assert_eq!(
            u16::from_le_bytes([img[base + 0x1a], img[base + 0x1b]]),
            4999
        );
        let flags = u16::from_le_bytes([img[base + 0x1c], img[base + 0x1d]]);
        assert_eq!(flags, 0xa000 | 0x3f << 6 | 1 << 5 | 0x10 | 3);
        assert_eq!(img[base + 0xc], 1);
        // The secret series has its own table; a zero time is never beaten.
        assert!(!best_stage(&mut img, true, 0, 1, &faster));
        img[0x175e + 30 + 0x1a..0x175e + 30 + 0x1c].copy_from_slice(&0xffffu16.to_le_bytes());
        assert!(best_stage(&mut img, true, 0, 1, &faster));
        assert_eq!(&img[0x175e + 30..0x175e + 35], b"COLIN");
    }

    #[test]
    fn a_rally_time_inserts_and_shifts() {
        let mut img = img();
        let base = 0xd7c + 0x50;
        for (place, t) in [1000u32, 2000, 3000, 4000, 5000].into_iter().enumerate() {
            let at = base + place * 16;
            img[at] = b'A' + place as u8;
            img[at + 0xc..at + 0x10].copy_from_slice(&(t | 0x80_0000).to_le_bytes());
        }
        assert_eq!(best_rally(&mut img, 1, b"NEW", 2, 6000, false, 0), None);
        assert_eq!(
            best_rally(&mut img, 1, b"NEW", 2, 2000, true, 1),
            Some(2),
            "ties keep their place"
        );
        assert_eq!(img[base + 3 * 16], b'C', "shifted down");
        assert_eq!(img[base + 4 * 16], b'D', "the fifth dropped off");
        let at = base + 2 * 16;
        let d = u32::from_le_bytes([img[at + 0xc], img[at + 0xd], img[at + 0xe], img[at + 0xf]]);
        assert_eq!(d, 1 << 25 | 1 << 24 | 2 << 19 | 0x80_0000 | 2000);
        assert_eq!(&img[at..at + 4], b"NEW\0");
    }

    #[test]
    fn a_championship_score_ranks_by_points_then_count() {
        let mut img = img();
        let base = 0xc98 + 75;
        img[base + 0xc] = 0x80 | 50;
        img[base + 0xd] = 3;
        assert_eq!(
            championship(&mut img, b"X", 1, 3, 50, 1, false, 0),
            Some(1),
            "equal count loses"
        );
        assert_eq!(championship(&mut img, b"Y", 2, 4, 50, 1, true, 5), Some(0));
        assert_eq!(img[base + 0xd], 0x24);
        assert_eq!(img[base + 0xc], 0x80 | 50);
        assert_eq!(img[base + 0xe], 5 << 1 | 1);
        assert_eq!(img[base + 15 + 0xd], 3, "the old leader shifted to second");
        let mut live = vec![0u8; 0x40];
        mark_live(&mut live);
        assert_eq!((live[0x18], live[0x19]), (0x40, 1));
    }
}
