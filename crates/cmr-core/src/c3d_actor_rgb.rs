//! Pure pre-draw Type-2 actor-RGB mutation.
//!
//! The shipped actor renderer optionally replaces diffuse RGB immediately
//! after its independent word-5 mutation. Prepared vertices are owned copies,
//! so this reconstruction never mutates retained source commands.

use crate::c3d_type2_fog::C3dType2PreDrawVertex;

/// Explicit retained actor-local RGB bytes consumed when the process-global
/// actor-RGB gate is enabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dActorRgbState<'a> {
    Disabled,
    /// One runtime RGB tuple per actor in source/group-0 order.
    Enabled(&'a [[u8; 3]]),
}

impl C3dActorRgbState<'_> {
    #[must_use]
    pub const fn enabled(self) -> bool {
        matches!(self, Self::Enabled(_))
    }

    #[must_use]
    pub const fn actor_count(self) -> Option<usize> {
        match self {
            Self::Disabled => None,
            Self::Enabled(actors) => Some(actors.len()),
        }
    }

    #[must_use]
    pub fn rgb(self, actor_index: usize) -> Option<[u8; 3]> {
        match self {
            Self::Disabled => None,
            Self::Enabled(actors) => actors.get(actor_index).copied(),
        }
    }
}

/// Replaces diffuse RGB while preserving the existing alpha byte and every
/// other Type-2 vertex word.
pub fn apply_c3d_actor_rgb_override(vertices: &mut [C3dType2PreDrawVertex], rgb: [u8; 3]) {
    let packed_rgb = u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2]);
    for vertex in vertices {
        vertex.replace_word4_rgb(packed_rgb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_rgb_and_preserves_diffuse_alpha() {
        let source = [
            0x0102_0304,
            0x1112_1314,
            0x2122_2324,
            0x3132_3334,
            0xa1de_adbe,
            0x5152_5354,
            0x6162_6364,
            0x7172_7374,
        ];
        let mut vertices = [C3dType2PreDrawVertex::from_words_for_test(source)];

        apply_c3d_actor_rgb_override(&mut vertices, [0x12, 0x34, 0x56]);

        let mut expected = source;
        expected[4] = 0xa112_3456;
        assert_eq!(vertices[0].words(), expected);
    }

    #[test]
    fn empty_input_is_a_no_op() {
        apply_c3d_actor_rgb_override(&mut [], [1, 2, 3]);
    }
}
