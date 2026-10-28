//! The cmRally.cfg record image and the live config block inside it.
//!
//! The record is the `0x1e20`-byte image at `PTR_DAT_0050622c`
//! ([`crate::config::LegacyConfig`]); `FUN_004b98b0` points the live block
//! `DAT_00d3ca80` at `record + 0x5d4`, so both live in one image. The front
//! end edits the record; [`commit_record_to_live`] (`FUN_004b93f0`) copies it
//! into the live block the race reads, and [`return_live_to_record`]
//! (`FUN_004b9600`) copies the race's results back when the front end
//! returns. [`apply_defaults`] is the reset `FUN_004b98b0` makes for a record
//! with a bad magic, size or token.
//!
//! Every write is the native byte operation. A description of disassembly, not
//! original source. Evidence: `rea.official-eu.config-commit`.

use crate::config::LEGACY_CONFIG_SIZE;
use crate::event_flow::SPECIAL_COUNTRIES;

/// The live block's offset in the record image (`FUN_004b98b0`: `param + 0x175`
/// dwords).
pub const LIVE_OFFSET: usize = 0x5d4;
/// The bytes `FUN_004b9e50` zeroes at the live block (`0x1b1` dwords).
pub const LIVE_LEN: usize = 0x6c4;
/// The record magic (`-0x789aedcc`).
pub const RECORD_MAGIC: u32 = 0x8765_1234;

/// The three master volumes the commit sets (`FUN_0047eb40/50/60`), each a
/// `live` byte's low seven bits times 20.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommitMasters {
    /// `DAT_004f6538` from `live[6]`.
    pub master_4f6538: i32,
    /// `DAT_004f653c` (the effects master) from `live[5]`.
    pub effects_4f653c: i32,
    /// `DAT_004f6540` from `live[7]`.
    pub master_4f6540: i32,
}

fn split(image: &mut [u8; LEGACY_CONFIG_SIZE]) -> (&mut [u8], &mut [u8]) {
    image.split_at_mut(LIVE_OFFSET)
}

fn copy(dst: &mut [u8], to: usize, src: &[u8], from: usize, len: usize) {
    dst[to..to + len].copy_from_slice(&src[from..from + len]);
}

fn splice(byte: u8, from: u8, mask: u8) -> u8 {
    ((byte ^ from) & mask) ^ byte
}

/// `FUN_004b93f0(record, live)`. In the attract demo (`record[8] & 1`) the
/// event fields are left alone. The calls to `FUN_00498b20`, `FUN_004b8cb0`
/// and `FUN_004cfab0` are not modelled.
pub fn commit_record_to_live(image: &mut [u8; LEGACY_CONFIG_SIZE]) -> CommitMasters {
    let (rec, live) = split(image);
    rec[0x20] &= 0x3f;
    rec[0x21..0x24].fill(0);
    copy(live, 0x04, rec, 0x0c, 0x1c);
    copy(live, 0x3c, rec, 0x44, 0x54);
    copy(live, 0xd8, rec, 0xe0, 0x54);
    copy(live, 0x20, rec, 0x28, 2);
    live[0x22] = rec[0x2a];
    live[0x23] = splice(live[0x23], rec[0x2b], 0x0e);
    live[0x24] = splice(live[0x24], rec[0x2c], 0x3f);
    live[0x00] = rec[0x08];
    if rec[0x08] & 1 == 0 {
        copy(live, 0x2c, rec, 0x34, 0x138);
        let country = usize::from(rec[0x181] & 0xf);
        copy(live, 0x164, rec, 0x16c + country * 2, 2);
        copy(live, 0x26c, rec, 0x184, 0x450);
        let mode = splice(live[0x166], rec[0x180], 3);
        let mode = (mode & 0xe3) | ((rec[0x182] & 7) << 2);
        live[0x168] = rec[0x181];
        let mode = (mode & 0xdf) | ((rec[0x182] & 0x10) << 1);
        live[0x166] = (mode & 0xbf) | ((rec[0x182] & 0x08) << 3);
    }
    live[0x167] &= 0xc0;
    live[0x166] &= 0x7f;
    let master = |byte: u8| i32::from(byte & 0x7f) * 20;
    CommitMasters {
        master_4f6538: master(live[6]),
        effects_4f653c: master(live[5]),
        master_4f6540: master(live[7]),
    }
}

/// `FUN_004b9600(live, record)`: copy the settings and the event back, keep
/// every unlock bit the race set, and clear the demo and rally-school
/// hand-off flags.
pub fn return_live_to_record(image: &mut [u8; LEGACY_CONFIG_SIZE]) {
    let (rec, live) = split(image);
    copy(rec, 0x0c, live, 0x04, 0x1c);
    copy(rec, 0x34, live, 0x2c, 0x138);
    // The unlock bits are ORed one at a time, in this order.
    for (offset, mask) in [
        (0x2b, 0x01),
        (0x2c, 0xc0),
        (0x2d, 0xff),
        (0x2e, 0xff),
        (0x2f, 0x0f),
        (0x30, 0x02),
        (0x2f, 0xf0),
        (0x30, 0x09),
    ] {
        rec[offset] |= live[offset - 8] & mask;
    }
    copy(rec, 0x28, live, 0x20, 2);
    rec[0x2a] = live[0x22];
    rec[0x2b] = splice(rec[0x2b], live[0x23], 0x0e);
    rec[0x2c] = splice(rec[0x2c], live[0x24], 0x3f);
    rec[0x08] &= 0xfe;
    rec[0x182] &= 0xdf;
}

/// What the defaults read from the host besides the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultsHost {
    /// The record token (`FUN_004cfac0` three times, `| 0xaa000000`).
    pub token: u32,
    /// The language `FUN_00496890` returns (low three bits used).
    pub language: u8,
    /// `FUN_004ad240() > 0xbcd3d80`.
    pub large_memory: bool,
}

/// `FUN_004b98b0`'s reset of a record with a bad magic, size or token, then
/// its commit. Not modelled: the two player blocks (the computer name, the
/// default control bindings `FUN_0049d210`) and the `0x2e6`-dword default
/// table copied from `0x506230` to `+0xc98`, which is shipped data the caller
/// supplies through `default_table` when it has it.
pub fn apply_defaults(
    image: &mut [u8; LEGACY_CONFIG_SIZE],
    host: DefaultsHost,
    default_table: Option<&[u8]>,
) -> CommitMasters {
    let put32 = |image: &mut [u8; LEGACY_CONFIG_SIZE], at: usize, value: u32| {
        image[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    put32(image, 0, RECORD_MAGIC);
    put32(image, 0x1e18, LEGACY_CONFIG_SIZE as u32);
    put32(image, 4, host.token);
    put32(image, 0x1e1c, !host.token);
    image[LIVE_OFFSET..LIVE_OFFSET + LIVE_LEN].fill(0);
    image[0x22] = 0;
    image[0x20] = ((image[0x20] & 0xe8) ^ (host.language & 7)) | 8;
    image[0x24] = 0;
    image[0xc95] &= 0xe0;
    image[0x20] &= 0x3f;
    image[0x21] = 0;
    image[0x23] = 0;
    image[0x20] |= 0x20;
    image[0x08] &= 0xfe;
    image[0xc94] = 0;
    image[0x10] = (image[0x10] & 0xf7) | 0x14;
    image[0x10] = if host.large_memory {
        image[0x10] | 0x20
    } else {
        image[0x10] & 0xdf
    };
    image[0x10] = (image[0x10] & 0xfe) | 0xc0;
    for at in [0x0d, 0x0f, 0x0e] {
        image[at] = (image[at] & 0x83) | 3;
    }
    image[0x2c] &= 0x80;
    image[0x30] &= 0xf4;
    image[0x11] = 0;
    put32(image, 0x1c, 0);
    image[0x12..0x19].copy_from_slice(&[0, 2, 1, 3, 2, 2, 1]);
    image[0x0c] = 0x85;
    image[0x2a] = 0x11;
    image[0x2b] &= 0xf0;
    image[0x2d..0x30].fill(0);
    image[0x28..0x2a].fill(0);
    image[0x182] &= 0xc0;
    image[0x181] = 0;
    image[0x180] = 0;
    // Every country selects no stage; its count is 6, or 7 for the special
    // countries; the two extra events (8, 9) have 4.
    for country in 0..10u8 {
        let at = 0x16c + usize::from(country) * 2;
        image[at] = 0;
        image[at + 1] = if country >= 8 {
            (image[at + 1] & 0xf4) | 4
        } else if SPECIAL_COUNTRIES.contains(&country) {
            (image[at + 1] & 0xf6) | 7
        } else {
            (image[at + 1] & 0xf6) | 6
        };
    }
    for record in 0..0x12 {
        put32(image, 0x1834 + record * 0x54, u32::MAX);
    }
    for slot in 0..7 {
        let at = 0x21c + slot * 0x9c;
        image[at - 4..at + 0xc].fill(0);
        image[at - 0x90] = 0;
    }
    if let Some(table) = default_table {
        let len = table.len().min(0x2e6 * 4);
        image[0xc98..0xc98 + len].copy_from_slice(&table[..len]);
    }
    commit_record_to_live(image)
}

/// The live block of the image.
pub fn live(image: &[u8; LEGACY_CONFIG_SIZE]) -> &[u8] {
    &image[LIVE_OFFSET..LIVE_OFFSET + LIVE_LEN]
}

/// The live block of the image.
pub fn live_mut(image: &mut [u8; LEGACY_CONFIG_SIZE]) -> &mut [u8] {
    &mut image[LIVE_OFFSET..LIVE_OFFSET + LIVE_LEN]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Box<[u8; LEGACY_CONFIG_SIZE]> {
        Box::new([0; LEGACY_CONFIG_SIZE])
    }

    #[test]
    fn the_commit_copies_the_event_and_settings() {
        let mut image = image();
        image[0x0c] = 0x85;
        image[0x0d] = 0x83;
        image[0x20] = 0xc8;
        image[0x21] = 9;
        image[0x2a] = 0x31;
        image[0x2b] = 0xff;
        image[0x2c] = 0xff;
        image[0x44] = 0x5a;
        image[0xbe] = 0x03;
        image[0x180] = 2;
        image[0x181] = 0x23;
        image[0x182] = 0x1d;
        image[0x16c + 3 * 2] = 0b101;
        image[0x16c + 3 * 2 + 1] = 7;
        image[0x184] = 0x77;
        let live_off = LIVE_OFFSET;
        image[live_off + 0x166] = 0x80;
        image[live_off + 0x167] = 0xc5;
        let masters = commit_record_to_live(&mut image);
        let live = live(&image);
        assert_eq!(image[0x20], 0x08, "the record's language byte is masked");
        assert_eq!(image[0x21], 0);
        assert_eq!(live[0x04], 0x85);
        assert_eq!(live[0x05], 0x83);
        assert_eq!(live[0x18], 0x08);
        assert_eq!(live[0x3c], 0x5a);
        assert_eq!(live[0xb6], 0x03, "the car nibble lands 8 bytes lower");
        assert_eq!(live[0x22], 0x31);
        assert_eq!(live[0x23], 0x0e);
        assert_eq!(live[0x24], 0x3f);
        assert_eq!((live[0x164], live[0x165]), (0b101, 7));
        assert_eq!(live[0x168], 0x23);
        // mode 2, arrangement 0x1d & 7 = 5, special bit 4 -> 0x20, secret bit 3 -> 0x40.
        assert_eq!(live[0x166], 2 | (5 << 2) | 0x20 | 0x40);
        assert_eq!(live[0x167], 0xc0);
        assert_eq!(live[0x26c], 0x77);
        assert_eq!(masters.effects_4f653c, 3 * 20);
    }

    #[test]
    fn the_demo_keeps_its_event() {
        let mut image = image();
        image[0x08] = 1;
        image[0x180] = 2;
        image[0x181] = 0x23;
        image[LIVE_OFFSET + 0x168] = 0x13;
        image[LIVE_OFFSET + 0x166] = 0x82;
        commit_record_to_live(&mut image);
        let live = live(&image);
        assert_eq!(live[0x00], 1);
        assert_eq!(live[0x168], 0x13);
        assert_eq!(live[0x166], 0x02);
    }

    #[test]
    fn the_return_keeps_unlocks_and_clears_the_hand_off_flags() {
        let mut image = image();
        image[0x08] = 0x03;
        image[0x182] = 0x25;
        image[0x2b] = 0x10;
        image[0x2c] = 0x01;
        let live_off = LIVE_OFFSET;
        image[live_off + 0x04] = 0x42;
        image[live_off + 0x2c] = 0x99;
        image[live_off + 0x23] = 0x0b;
        image[live_off + 0x24] = 0xc5;
        image[live_off + 0x25] = 0xa5;
        image[live_off + 0x27] = 0x3c;
        image[live_off + 0x28] = 0xff;
        image[live_off + 0x22] = 0x44;
        return_live_to_record(&mut image);
        assert_eq!(image[0x0c], 0x42);
        assert_eq!(image[0x34], 0x99);
        assert_eq!(image[0x2b], 0x10 | 0x01 | 0x0a);
        assert_eq!(image[0x2c], 0xc0 | 0x05);
        assert_eq!(image[0x2d], 0xa5);
        assert_eq!(image[0x2f], 0x3c);
        assert_eq!(image[0x30], 0x0b);
        assert_eq!(image[0x2a], 0x44);
        assert_eq!((image[0x08], image[0x182]), (0x02, 0x05));
    }

    #[test]
    fn the_defaults_seed_the_country_table_and_commit() {
        let mut image = image();
        image[0x16c + 2 * 2] = 0xff;
        image[0x16d + 2 * 2] = 0xf9;
        let masters = apply_defaults(
            &mut image,
            DefaultsHost {
                token: 0xaa12_3456,
                language: 2,
                large_memory: true,
            },
            None,
        );
        assert_eq!(&image[0..4], &RECORD_MAGIC.to_le_bytes());
        assert_eq!(&image[0x1e1c..0x1e20], &(!0xaa12_3456u32).to_le_bytes());
        assert_eq!(image[0x20], (2 | 8 | 0x20) & 0x3f);
        assert_eq!(image[0x10], 0x14 | 0x20 | 0xc0);
        assert_eq!(&image[0x0c..0x10], &[0x85, 3, 3, 3]);
        assert_eq!(&image[0x12..0x19], &[0, 2, 1, 3, 2, 2, 1]);
        assert_eq!((image[0x16c + 4], image[0x16d + 4]), (0, 0xf6));
        assert_eq!(image[0x16d + 2], 7, "country 1 is special");
        assert_eq!(image[0x16d + 16], 4);
        assert_eq!(&image[0x1834..0x1838], &[0xff; 4]);
        // The commit lands the defaults in the live block.
        let live = live(&image);
        assert_eq!(live[0x18], image[0x20]);
        assert_eq!((live[0x164], live[0x165]), (0, 6));
        assert_eq!(live[0x166] & 3, 0);
        assert_eq!(masters.effects_4f653c, 3 * 20);
    }
}
