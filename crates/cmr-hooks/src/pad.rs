//! Pad buttons and d-pad (E11, F23).
//!
//! The game reads a pad through WinMM and turns physical button n into its
//! button flag n (`0x10 << (n - 1)`); every menu, the button setup and the
//! race read those flags. F23 sits where the buttons are read and lets the
//! player choose which physical button feeds each of the game's buttons,
//! and adds the d-pad, which the game never asked WinMM for, as the four
//! direction flags the left stick sets (menus, and digital driving).

/// How many of the game's buttons can be remapped (it reads 12).
pub const GAME_BUTTONS: usize = 12;

/// The buttons the game sees: game button `k` (bit `k`) is down when the
/// physical button `map[k]` is (1-based; 0 keeps physical button `k + 1`).
/// Buttons past the twelfth pass through.
pub fn remap(buttons: u32, map: &[u8; GAME_BUTTONS]) -> u32 {
    let mut out = buttons & !((1 << GAME_BUTTONS) - 1);
    for (k, &m) in map.iter().enumerate() {
        let physical = if m == 0 { k as u32 } else { u32::from(m) - 1 };
        if physical < 32 && buttons >> physical & 1 != 0 {
            out |= 1 << k;
        }
    }
    out
}

/// The game's direction flags (left 1, right 2, up 4, down 8, as the stick
/// sets them) for a WinMM point-of-view hat in hundredths of a degree
/// (0 up, 9000 right; 0xffff when centred). Diagonals set two.
pub fn pov_directions(pov: u32) -> u32 {
    if pov >= 36000 {
        return 0;
    }
    let within = |centre: u32| {
        let d = pov.abs_diff(centre);
        d.min(36000 - d) <= 6750
    };
    let mut flags = 0;
    if within(27000) {
        flags |= 1;
    }
    if within(9000) {
        flags |= 2;
    }
    if within(0) {
        flags |= 4;
    }
    if within(18000) {
        flags |= 8;
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_map_keeps_the_buttons() {
        assert_eq!(remap(0b1010_0101, &[0; GAME_BUTTONS]), 0b1010_0101);
        assert_eq!(
            remap(1 << 20, &[0; GAME_BUTTONS]),
            1 << 20,
            "past the twelfth"
        );
    }

    #[test]
    fn one_physical_button_can_feed_several_game_buttons() {
        // Game button 5 (handbrake) and 1 (select) both from A (button 1);
        // game button 3 from Y (4).
        let mut map = [0; GAME_BUTTONS];
        map[4] = 1;
        map[2] = 4;
        let a = 1;
        assert_eq!(remap(a, &map), 1 | 1 << 4);
        // Y feeds game button 3, and its own game button 4 still follows
        // physical button 4 (unmapped entries keep their own).
        let y = 1 << 3;
        assert_eq!(remap(y, &map), 1 << 2 | 1 << 3);
        // X (3) no longer reaches game button 3.
        assert_eq!(remap(1 << 2, &map), 0);
    }

    #[test]
    fn the_hat_gives_directions() {
        assert_eq!(pov_directions(0xffff), 0, "centred");
        assert_eq!(pov_directions(0), 4);
        assert_eq!(pov_directions(9000), 2);
        assert_eq!(pov_directions(18000), 8);
        assert_eq!(pov_directions(27000), 1);
        assert_eq!(pov_directions(4500), 4 | 2);
        assert_eq!(pov_directions(31500), 4 | 1);
        assert_eq!(pov_directions(22500), 8 | 1);
    }
}
