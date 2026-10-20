//! Deferred active-palette selection and terminal-frame commit.
//!
//! Static analysis recovered a queue boundary that compares a resolved palette
//! arena index with the current index, plus an 8-bit-only terminal commit. The
//! logical mapper uses runtime-loaded group bases rather than inventing fixed
//! arena offsets. Adapter/tool failures are exposed as typed errors; leaving
//! project-owned state and caches unchanged on such a failure is a
//! reconstruction policy, not observed native error handling.

use std::fmt;

use cmr_formats::riff_palette::PaletteEntry;

/// One already-resolved entry in the loaded 256-entry palette arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteArenaIndex(i32);

impl PaletteArenaIndex {
    pub const fn new(index: i32) -> Self {
        Self(index)
    }

    pub const fn get(self) -> i32 {
        self.0
    }
}

/// Runtime-loaded base indexes used by the recovered logical palette mapper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteGroupBases {
    pub group_1: i32,
    pub group_2: i32,
    pub group_3: i32,
    pub group_4: i32,
    /// Group 5 selects this singleton and ignores its item argument.
    pub group_5: i32,
}

/// Resolves the native `(item, group)` request to an arena index.
///
/// Groups 1 through 4 use their runtime-loaded base plus `item`, with native
/// 32-bit wrapping arithmetic. Group 5 selects its singleton. Every other
/// group value returns `item` unchanged.
pub const fn resolve_palette_index(
    item: i32,
    group: i32,
    bases: PaletteGroupBases,
) -> PaletteArenaIndex {
    PaletteArenaIndex::new(match group {
        1 => bases.group_1.wrapping_add(item),
        2 => bases.group_2.wrapping_add(item),
        3 => bases.group_3.wrapping_add(item),
        4 => bases.group_4.wrapping_add(item),
        5 => bases.group_5,
        _ => item,
    })
}

/// Recovered current/pending state. `None` models the native `-1` sentinel.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeferredPaletteState {
    current: Option<PaletteArenaIndex>,
    pending: Option<PaletteArenaIndex>,
}

impl DeferredPaletteState {
    /// Models the native initialization before its first logical request.
    pub const fn new() -> Self {
        Self {
            current: None,
            pending: None,
        }
    }

    pub const fn current(&self) -> Option<PaletteArenaIndex> {
        self.current
    }

    pub const fn pending(&self) -> Option<PaletteArenaIndex> {
        self.pending
    }

    /// Queues a resolved palette unless it is already current.
    ///
    /// Native code compares only against `current`, not against `pending`.
    /// Consequently, requesting the current palette does not cancel a distinct
    /// request that is already pending.
    pub fn request(&mut self, requested: PaletteArenaIndex) -> PaletteRequestOutcome {
        let requested = (requested.get() != -1).then_some(requested);
        if self.current == requested {
            PaletteRequestOutcome::CurrentMatch
        } else {
            self.pending = requested;
            PaletteRequestOutcome::PendingUpdated
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaletteRequestOutcome {
    PendingUpdated,
    CurrentMatch,
}

/// Platform adapter for the recovered 256-entry display-palette upload.
pub trait DeferredPaletteAdapter {
    type Error;

    /// Invokes the native upload for all 256 entries.
    ///
    /// The shipped routine does not branch on the native method's result. A
    /// completed invocation should therefore return `Ok(())` even if its
    /// native status reports failure. `Err` is reserved for an adapter/tool
    /// failure that prevents the invocation from being performed or reported.
    fn upload_palette(&mut self, entries: &[PaletteEntry; 256]) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredPaletteCommitOutcome {
    SkippedNonIndexedDepth,
    NoPendingPalette,
    Committed(PaletteArenaIndex),
}

#[derive(Debug, Eq, PartialEq)]
pub enum DeferredPaletteCommitError<E> {
    MissingArenaEntry {
        index: PaletteArenaIndex,
        palette_count: usize,
    },
    Adapter(E),
}

impl<E: fmt::Display> fmt::Display for DeferredPaletteCommitError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingArenaEntry {
                index,
                palette_count,
            } => write!(
                formatter,
                "palette arena index {} is outside {palette_count} loaded palettes",
                index.get()
            ),
            Self::Adapter(source) => write!(formatter, "palette upload failed: {source}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for DeferredPaletteCommitError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Adapter(source) => Some(source),
            Self::MissingArenaEntry { .. } => None,
        }
    }
}

/// Commits a pending palette at the terminal frame boundary when depth is 8.
///
/// On success, the recovered order is upload, renderer-cache copy, global-cache
/// copy, current-index update, then pending-index clear. Checked arena lookup
/// and fail-fast adapter errors are source-owned safety policies. Those errors
/// leave the state and both software caches unchanged; an adapter is responsible
/// for documenting whether a failed upload can itself have external effects.
pub fn commit_deferred_palette<A: DeferredPaletteAdapter>(
    state: &mut DeferredPaletteState,
    bits_per_pixel: u8,
    arena: &[[PaletteEntry; 256]],
    renderer_cache: &mut [PaletteEntry; 256],
    global_cache: &mut [PaletteEntry; 256],
    adapter: &mut A,
) -> Result<DeferredPaletteCommitOutcome, DeferredPaletteCommitError<A::Error>> {
    if bits_per_pixel != 8 {
        return Ok(DeferredPaletteCommitOutcome::SkippedNonIndexedDepth);
    }

    let Some(pending) = state.pending else {
        return Ok(DeferredPaletteCommitOutcome::NoPendingPalette);
    };
    let entries = usize::try_from(pending.get())
        .ok()
        .and_then(|index| arena.get(index))
        .ok_or(DeferredPaletteCommitError::MissingArenaEntry {
            index: pending,
            palette_count: arena.len(),
        })?;

    adapter
        .upload_palette(entries)
        .map_err(DeferredPaletteCommitError::Adapter)?;
    *renderer_cache = *entries;
    *global_cache = *entries;
    state.current = Some(pending);
    state.pending = None;

    Ok(DeferredPaletteCommitOutcome::Committed(pending))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq)]
    struct UploadFailure;

    impl fmt::Display for UploadFailure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("injected upload failure")
        }
    }

    impl std::error::Error for UploadFailure {}

    #[derive(Default)]
    struct RecordingAdapter {
        uploads: Vec<[PaletteEntry; 256]>,
        fail: bool,
    }

    impl DeferredPaletteAdapter for RecordingAdapter {
        type Error = UploadFailure;

        fn upload_palette(&mut self, entries: &[PaletteEntry; 256]) -> Result<(), Self::Error> {
            self.uploads.push(*entries);
            if self.fail {
                Err(UploadFailure)
            } else {
                Ok(())
            }
        }
    }

    fn palette(seed: u8) -> [PaletteEntry; 256] {
        std::array::from_fn(|index| PaletteEntry {
            red: seed.wrapping_add(index as u8),
            green: seed.wrapping_add((index as u8).wrapping_mul(3)),
            blue: seed.wrapping_add((index as u8).wrapping_mul(7)),
            flags: index as u8,
        })
    }

    #[test]
    fn initialization_and_requests_preserve_current_pending_mapping() {
        let mut state = DeferredPaletteState::new();
        let first = resolve_palette_index(
            0,
            0,
            PaletteGroupBases {
                group_1: 10,
                group_2: 20,
                group_3: 30,
                group_4: 40,
                group_5: 50,
            },
        );
        let replacement = PaletteArenaIndex::new(9);

        assert_eq!(first, PaletteArenaIndex::new(0));
        assert_eq!(state.current(), None);
        assert_eq!(state.pending(), None);
        assert_eq!(state.request(first), PaletteRequestOutcome::PendingUpdated);
        assert_eq!(state.current(), None);
        assert_eq!(state.pending(), Some(first));
        assert_eq!(
            state.request(replacement),
            PaletteRequestOutcome::PendingUpdated
        );
        assert_eq!(state.current(), None);
        assert_eq!(state.pending(), Some(replacement));
    }

    #[test]
    fn logical_mapper_uses_runtime_bases_singleton_and_default_item() {
        let bases = PaletteGroupBases {
            group_1: 10,
            group_2: 20,
            group_3: 30,
            group_4: 40,
            group_5: 99,
        };

        assert_eq!(
            resolve_palette_index(3, 1, bases),
            PaletteArenaIndex::new(13)
        );
        assert_eq!(
            resolve_palette_index(3, 2, bases),
            PaletteArenaIndex::new(23)
        );
        assert_eq!(
            resolve_palette_index(3, 3, bases),
            PaletteArenaIndex::new(33)
        );
        assert_eq!(
            resolve_palette_index(3, 4, bases),
            PaletteArenaIndex::new(43)
        );
        assert_eq!(
            resolve_palette_index(3, 5, bases),
            PaletteArenaIndex::new(99)
        );
        assert_eq!(
            resolve_palette_index(7, 5, bases),
            PaletteArenaIndex::new(99)
        );
        assert_eq!(
            resolve_palette_index(3, 0, bases),
            PaletteArenaIndex::new(3)
        );
        assert_eq!(
            resolve_palette_index(3, 6, bases),
            PaletteArenaIndex::new(3)
        );
        assert_eq!(
            resolve_palette_index(
                1,
                1,
                PaletteGroupBases {
                    group_1: i32::MAX,
                    ..bases
                }
            ),
            PaletteArenaIndex::new(i32::MIN)
        );
    }

    #[test]
    fn request_deduplicates_only_against_current_and_does_not_cancel_pending() {
        let current = PaletteArenaIndex::new(0);
        let pending = PaletteArenaIndex::new(1);
        let arena = [palette(10), palette(20)];
        let mut renderer_cache = palette(1);
        let mut global_cache = palette(2);
        let mut adapter = RecordingAdapter::default();
        let mut state = DeferredPaletteState::new();
        state.request(current);
        commit_deferred_palette(
            &mut state,
            8,
            &arena,
            &mut renderer_cache,
            &mut global_cache,
            &mut adapter,
        )
        .unwrap();

        assert_eq!(state.request(current), PaletteRequestOutcome::CurrentMatch);
        assert_eq!(state.pending(), None);
        assert_eq!(
            state.request(pending),
            PaletteRequestOutcome::PendingUpdated
        );
        assert_eq!(state.request(current), PaletteRequestOutcome::CurrentMatch);
        assert_eq!(state.pending(), Some(pending));
    }

    #[test]
    fn raw_minus_one_request_obeys_the_native_current_and_pending_sentinels() {
        let sentinel = PaletteArenaIndex::new(-1);
        let pending = PaletteArenaIndex::new(1);
        let mut state = DeferredPaletteState::new();

        state.request(pending);
        assert_eq!(state.request(sentinel), PaletteRequestOutcome::CurrentMatch);
        assert_eq!(state.pending(), Some(pending));

        let arena = [palette(10), palette(20)];
        let mut renderer_cache = palette(1);
        let mut global_cache = palette(2);
        let mut adapter = RecordingAdapter::default();
        commit_deferred_palette(
            &mut state,
            8,
            &arena,
            &mut renderer_cache,
            &mut global_cache,
            &mut adapter,
        )
        .unwrap();
        assert_eq!(state.current(), Some(pending));

        assert_eq!(
            state.request(sentinel),
            PaletteRequestOutcome::PendingUpdated
        );
        assert_eq!(state.pending(), None);
    }

    #[test]
    fn depth_eight_uploads_and_caches_then_records_current_and_clears_pending() {
        let selected = PaletteArenaIndex::new(1);
        let arena = [palette(10), palette(80)];
        let mut renderer_cache = palette(1);
        let mut global_cache = palette(2);
        let mut adapter = RecordingAdapter::default();
        let mut state = DeferredPaletteState::new();
        state.request(selected);

        let outcome = commit_deferred_palette(
            &mut state,
            8,
            &arena,
            &mut renderer_cache,
            &mut global_cache,
            &mut adapter,
        )
        .unwrap();

        assert_eq!(outcome, DeferredPaletteCommitOutcome::Committed(selected));
        assert_eq!(adapter.uploads, vec![arena[1]]);
        assert_eq!(renderer_cache, arena[1]);
        assert_eq!(global_cache, arena[1]);
        assert_eq!(state.current(), Some(selected));
        assert_eq!(state.pending(), None);
    }

    #[test]
    fn no_pending_palette_is_a_complete_no_op() {
        let original_renderer = palette(1);
        let original_global = palette(2);
        let mut renderer_cache = original_renderer;
        let mut global_cache = original_global;
        let mut adapter = RecordingAdapter::default();
        let mut state = DeferredPaletteState::new();

        let outcome = commit_deferred_palette(
            &mut state,
            8,
            &[palette(10)],
            &mut renderer_cache,
            &mut global_cache,
            &mut adapter,
        )
        .unwrap();

        assert_eq!(outcome, DeferredPaletteCommitOutcome::NoPendingPalette);
        assert!(adapter.uploads.is_empty());
        assert_eq!(renderer_cache, original_renderer);
        assert_eq!(global_cache, original_global);
        assert_eq!(state, DeferredPaletteState::new());
    }

    #[test]
    fn non_eight_bit_depth_preserves_pending_and_has_no_effects() {
        let requested = PaletteArenaIndex::new(0);
        let original_renderer = palette(1);
        let original_global = palette(2);
        let mut renderer_cache = original_renderer;
        let mut global_cache = original_global;
        let mut adapter = RecordingAdapter::default();
        let mut state = DeferredPaletteState::new();
        state.request(requested);

        let outcome = commit_deferred_palette(
            &mut state,
            16,
            &[palette(10)],
            &mut renderer_cache,
            &mut global_cache,
            &mut adapter,
        )
        .unwrap();

        assert_eq!(
            outcome,
            DeferredPaletteCommitOutcome::SkippedNonIndexedDepth
        );
        assert!(adapter.uploads.is_empty());
        assert_eq!(renderer_cache, original_renderer);
        assert_eq!(global_cache, original_global);
        assert_eq!(state.current(), None);
        assert_eq!(state.pending(), Some(requested));
    }

    #[test]
    fn missing_arena_entry_is_typed_and_preserves_all_owned_state() {
        let requested = PaletteArenaIndex::new(3);
        let original_renderer = palette(1);
        let original_global = palette(2);
        let mut renderer_cache = original_renderer;
        let mut global_cache = original_global;
        let mut adapter = RecordingAdapter::default();
        let mut state = DeferredPaletteState::new();
        state.request(requested);

        let error = commit_deferred_palette(
            &mut state,
            8,
            &[palette(10)],
            &mut renderer_cache,
            &mut global_cache,
            &mut adapter,
        )
        .unwrap_err();

        assert_eq!(
            error,
            DeferredPaletteCommitError::MissingArenaEntry {
                index: requested,
                palette_count: 1,
            }
        );
        assert!(adapter.uploads.is_empty());
        assert_eq!(renderer_cache, original_renderer);
        assert_eq!(global_cache, original_global);
        assert_eq!(state.current(), None);
        assert_eq!(state.pending(), Some(requested));
    }

    #[test]
    fn adapter_failure_is_typed_and_preserves_owned_state_and_caches() {
        let requested = PaletteArenaIndex::new(0);
        let original_renderer = palette(1);
        let original_global = palette(2);
        let mut renderer_cache = original_renderer;
        let mut global_cache = original_global;
        let mut adapter = RecordingAdapter {
            fail: true,
            ..RecordingAdapter::default()
        };
        let mut state = DeferredPaletteState::new();
        state.request(requested);

        let error = commit_deferred_palette(
            &mut state,
            8,
            &[palette(10)],
            &mut renderer_cache,
            &mut global_cache,
            &mut adapter,
        )
        .unwrap_err();

        assert_eq!(error, DeferredPaletteCommitError::Adapter(UploadFailure));
        assert_eq!(adapter.uploads, vec![palette(10)]);
        assert_eq!(renderer_cache, original_renderer);
        assert_eq!(global_cache, original_global);
        assert_eq!(state.current(), None);
        assert_eq!(state.pending(), Some(requested));
    }
}
