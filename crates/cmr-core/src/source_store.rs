//! Shared lifetime owner for persistent indexed source surfaces.
//!
//! Resource-specific pools keep opaque [`SourceKey`] values instead of each
//! borrowing the backend. This lets fonts, root-menu bitmaps, and later
//! frontend resources coexist while one store enforces checked resolution and
//! reverse-order cleanup.

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::blit::BlitPlan;
use crate::display::IndexedVideoBackend;
use crate::surface::IndexedSurface;

static NEXT_STORE_ID: AtomicU64 = AtomicU64::new(1);

fn allocate_store_id(counter: &AtomicU64) -> Option<u64> {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        if current == 0 {
            return None;
        }
        let next = current.checked_add(1).unwrap_or(0);
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return Some(current),
            Err(observed) => current = observed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceKey {
    store_id: u64,
    slot: NonZeroUsize,
}

/// Backend-neutral reference to one indexed bitmap uploaded into a source
/// store.
///
/// The store owns the backend resource. This copyable value retains only the
/// store-scoped source key and the exact uploaded surface extent, so it can be
/// shared by screen-specific resource pools without exposing a backend handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadedIndexedBitmap {
    source: SourceKey,
    width: u32,
    height: u32,
}

impl UploadedIndexedBitmap {
    #[must_use]
    pub const fn source_key(self) -> SourceKey {
        self.source
    }

    #[must_use]
    pub const fn extent(self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Checks that this bitmap still belongs to `store` and remains live,
    /// without submitting a backend operation.
    pub fn validate_source<B: IndexedVideoBackend>(
        self,
        store: &UploadedSourceStore<'_, B>,
    ) -> Result<(), SourceAccessError> {
        store.validate_source(self.source)
    }

    /// Submits an already-validated blit through the owning source store.
    ///
    /// Store identity and source lifetime remain checked by
    /// [`UploadedSourceStore::blit`]. Geometry validation is intentionally the
    /// caller's responsibility because different screens recover different
    /// clipping and color-key policies.
    pub fn blit<B: IndexedVideoBackend>(
        self,
        store: &mut UploadedSourceStore<'_, B>,
        destination: B::Surface,
        plan: BlitPlan,
    ) -> Result<(), SourceBlitError<B::Error>> {
        store.blit(self.source, destination, plan)
    }
}

impl SourceKey {
    #[must_use]
    pub const fn slot(self) -> usize {
        self.slot.get() - 1
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum SourceUploadError<E> {
    Capacity,
    Backend(E),
}

impl<E: fmt::Display> fmt::Display for SourceUploadError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity => write!(formatter, "indexed source store capacity exhausted"),
            Self::Backend(error) => write!(formatter, "cannot create indexed source: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for SourceUploadError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(error) => Some(error),
            Self::Capacity => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceAccessError {
    ForeignKey(SourceKey),
    UnknownKey(SourceKey),
    ReleasedKey(SourceKey),
}

impl fmt::Display for SourceAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignKey(key) => {
                write!(formatter, "source key {key:?} belongs to another store")
            }
            Self::UnknownKey(key) => write!(formatter, "source key {key:?} was never allocated"),
            Self::ReleasedKey(key) => write!(formatter, "source key {key:?} has been released"),
        }
    }
}

impl std::error::Error for SourceAccessError {}

#[derive(Debug, Eq, PartialEq)]
pub enum SourceBlitError<E> {
    Access(SourceAccessError),
    Backend(E),
}

impl<E: fmt::Display> fmt::Display for SourceBlitError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Access(error) => error.fmt(formatter),
            Self::Backend(error) => write!(formatter, "indexed source blit failed: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for SourceBlitError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Access(error) => Some(error),
            Self::Backend(error) => Some(error),
        }
    }
}

/// Owns every uploaded indexed source associated with one backend borrow.
///
/// Keys are store-specific and slots are never reused, so a stale or foreign
/// key cannot silently resolve to another surface. Drop clears each live slot
/// before destroying it in reverse creation order. Backend implementations are
/// required to make `destroy_source` non-panicking.
pub struct UploadedSourceStore<'backend, B: IndexedVideoBackend> {
    backend: &'backend mut B,
    store_id: u64,
    slots: Vec<Option<B::Surface>>,
    creation_order: Vec<SourceKey>,
}

impl<'backend, B: IndexedVideoBackend> UploadedSourceStore<'backend, B> {
    #[must_use]
    pub fn new(backend: &'backend mut B) -> Self {
        let store_id = allocate_store_id(&NEXT_STORE_ID)
            .expect("indexed source store identity exhausted permanently");
        Self {
            backend,
            store_id,
            slots: Vec::new(),
            creation_order: Vec::new(),
        }
    }

    pub fn upload(
        &mut self,
        surface: &IndexedSurface,
    ) -> Result<SourceKey, SourceUploadError<B::Error>> {
        self.upload_inner(surface)
    }

    /// Uploads an indexed surface and retains its exact extent beside the
    /// resulting store-scoped key.
    pub fn upload_bitmap(
        &mut self,
        surface: &IndexedSurface,
    ) -> Result<UploadedIndexedBitmap, SourceUploadError<B::Error>> {
        let source = self.upload_inner(surface)?;
        Ok(UploadedIndexedBitmap {
            source,
            width: surface.width,
            height: surface.height,
        })
    }

    #[must_use]
    pub fn transaction(&mut self) -> SourceUploadTransaction<'_, 'backend, B> {
        let start = self.creation_order.len();
        SourceUploadTransaction {
            store: self,
            start,
            committed: false,
        }
    }

    pub fn blit(
        &mut self,
        source: SourceKey,
        destination: B::Surface,
        plan: BlitPlan,
    ) -> Result<(), SourceBlitError<B::Error>> {
        let source = self.resolve(source).map_err(SourceBlitError::Access)?;
        self.backend
            .blit(source, destination, plan)
            .map_err(SourceBlitError::Backend)
    }

    /// Validates a key's store identity and lifetime without touching the
    /// backend. Frame executors use this to reject all stale resources before
    /// beginning an otherwise ordered batch of effects.
    pub fn validate_source(&self, key: SourceKey) -> Result<(), SourceAccessError> {
        self.resolve(key).map(drop)
    }

    fn resolve(&self, key: SourceKey) -> Result<B::Surface, SourceAccessError> {
        if key.store_id != self.store_id {
            return Err(SourceAccessError::ForeignKey(key));
        }
        match self.slots.get(key.slot()) {
            None => Err(SourceAccessError::UnknownKey(key)),
            Some(None) => Err(SourceAccessError::ReleasedKey(key)),
            Some(Some(surface)) => Ok(*surface),
        }
    }

    pub fn release(&mut self, key: SourceKey) -> Result<(), SourceAccessError> {
        if key.store_id != self.store_id {
            return Err(SourceAccessError::ForeignKey(key));
        }
        let Some(slot) = self.slots.get_mut(key.slot()) else {
            return Err(SourceAccessError::UnknownKey(key));
        };
        let Some(surface) = slot.take() else {
            return Err(SourceAccessError::ReleasedKey(key));
        };
        self.backend.destroy_source(surface);
        Ok(())
    }

    #[must_use]
    pub fn uploaded_source_count(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// Returns the backend for read-only capability and diagnostic queries.
    #[must_use]
    pub fn backend(&self) -> &B {
        self.backend
    }

    /// Temporarily exposes the backend to another compatibility interface.
    ///
    /// Callers must not invalidate source surfaces owned by this store.
    pub(crate) fn backend_mut(&mut self) -> &mut B {
        self.backend
    }

    fn upload_inner(
        &mut self,
        surface: &IndexedSurface,
    ) -> Result<SourceKey, SourceUploadError<B::Error>> {
        let next = self
            .slots
            .len()
            .checked_add(1)
            .and_then(NonZeroUsize::new)
            .ok_or(SourceUploadError::Capacity)?;
        let uploaded = self
            .backend
            .create_source(surface)
            .map_err(SourceUploadError::Backend)?;
        let key = SourceKey {
            store_id: self.store_id,
            slot: next,
        };
        self.slots.push(Some(uploaded));
        self.creation_order.push(key);
        Ok(key)
    }

    fn rollback_to(&mut self, creation_count: usize) {
        while self.creation_order.len() > creation_count {
            let key = self
                .creation_order
                .pop()
                .expect("length check proves a source key remains");
            if let Some(surface) = self.slots[key.slot()].take() {
                self.backend.destroy_source(surface);
            }
        }
    }
}

impl<B: IndexedVideoBackend> Drop for UploadedSourceStore<'_, B> {
    fn drop(&mut self) {
        self.rollback_to(0);
    }
}

/// Unwind-safe batch upload. Dropping without [`Self::commit`] releases only
/// sources created since this transaction began, in reverse order.
pub struct SourceUploadTransaction<'store, 'backend, B: IndexedVideoBackend> {
    store: &'store mut UploadedSourceStore<'backend, B>,
    start: usize,
    committed: bool,
}

impl<B: IndexedVideoBackend> SourceUploadTransaction<'_, '_, B> {
    pub fn upload(
        &mut self,
        surface: &IndexedSurface,
    ) -> Result<SourceKey, SourceUploadError<B::Error>> {
        self.store.upload_inner(surface)
    }

    /// Transactional counterpart to [`UploadedSourceStore::upload_bitmap`].
    pub fn upload_bitmap(
        &mut self,
        surface: &IndexedSurface,
    ) -> Result<UploadedIndexedBitmap, SourceUploadError<B::Error>> {
        let source = self.store.upload_inner(surface)?;
        Ok(UploadedIndexedBitmap {
            source,
            width: surface.width,
            height: surface.height,
        })
    }

    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl<B: IndexedVideoBackend> Drop for SourceUploadTransaction<'_, '_, B> {
    fn drop(&mut self) {
        if !self.committed {
            self.store.rollback_to(self.start);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_identity_exhaustion_never_wraps_or_reuses_an_identity() {
        let counter = AtomicU64::new(u64::MAX - 1);
        assert_eq!(allocate_store_id(&counter), Some(u64::MAX - 1));
        assert_eq!(allocate_store_id(&counter), Some(u64::MAX));
        assert_eq!(allocate_store_id(&counter), None);
        assert_eq!(allocate_store_id(&counter), None);
        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }
    use crate::display::{CooperativeMode, DisplaySurfaces, IndexedMode, SurfaceTopology};
    use cmr_formats::riff_palette::PaletteEntry;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum BackendError {
        Create(usize),
        Blit,
    }

    impl fmt::Display for BackendError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{self:?}")
        }
    }

    impl std::error::Error for BackendError {}

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        Create(u8),
        Blit(u8),
        Destroy(u8),
    }

    #[derive(Default)]
    struct RecordingBackend {
        events: Vec<Event>,
        create_attempts: usize,
        fail_create_at: Option<usize>,
        panic_create_at: Option<usize>,
        fail_blit: bool,
    }

    impl IndexedVideoBackend for RecordingBackend {
        type Error = BackendError;
        type Surface = u8;
        type Palette = u8;

        fn create_device(&mut self) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_cooperative_mode(&mut self, _mode: CooperativeMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn set_display_mode(&mut self, _mode: IndexedMode) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_surfaces(
            &mut self,
            _topology: SurfaceTopology,
            _mode: IndexedMode,
        ) -> Result<DisplaySurfaces<Self::Surface>, Self::Error> {
            unreachable!()
        }

        fn create_palette(
            &mut self,
            _entries: &[PaletteEntry; 256],
        ) -> Result<Self::Palette, Self::Error> {
            unreachable!()
        }

        fn attach_palette(
            &mut self,
            _surface: Self::Surface,
            _palette: Self::Palette,
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn upload_palette(
            &mut self,
            _palette: Self::Palette,
            _entries: &[PaletteEntry; 256],
        ) -> Result<(), Self::Error> {
            unreachable!()
        }

        fn create_source(
            &mut self,
            _surface: &IndexedSurface,
        ) -> Result<Self::Surface, Self::Error> {
            let attempt = self.create_attempts;
            self.create_attempts += 1;
            assert_ne!(self.panic_create_at, Some(attempt), "injected create panic");
            if self.fail_create_at == Some(attempt) {
                return Err(BackendError::Create(attempt));
            }
            let handle = 10 + attempt as u8;
            self.events.push(Event::Create(handle));
            Ok(handle)
        }

        fn blit(
            &mut self,
            source: Self::Surface,
            _destination: Self::Surface,
            _plan: BlitPlan,
        ) -> Result<(), Self::Error> {
            self.events.push(Event::Blit(source));
            if self.fail_blit {
                Err(BackendError::Blit)
            } else {
                Ok(())
            }
        }

        fn destroy_source(&mut self, source: Self::Surface) {
            self.events.push(Event::Destroy(source));
        }
    }

    fn surface(marker: u8) -> IndexedSurface {
        IndexedSurface {
            width: 1,
            height: 1,
            pitch: 1,
            pixels: vec![marker],
            palette: [PaletteEntry {
                red: 0,
                green: 0,
                blue: 0,
                flags: 0,
            }; 256],
        }
    }

    fn one_pixel_blit() -> BlitPlan {
        let crate::blit::BlitDecision::Blit(plan) = crate::blit::plan_legacy_blit(
            crate::blit::RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            crate::blit::RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            1,
            1,
            None,
        ) else {
            unreachable!()
        };
        plan
    }

    #[test]
    fn uploaded_bitmap_retains_exact_extent_and_submits_through_store() {
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let mut source = surface(7);
            source.width = 4;
            source.height = 2;
            source.pitch = 4;
            source.pixels = vec![7; 8];
            let bitmap = store.upload_bitmap(&source).unwrap();

            assert_eq!(bitmap.extent(), (4, 2));
            assert_eq!(bitmap.source_key().slot(), 0);
            assert_eq!(bitmap.blit(&mut store, 99, one_pixel_blit()), Ok(()));
            assert_eq!(store.backend().events, [Event::Create(10), Event::Blit(10)]);
        }
        assert_eq!(backend.events.last(), Some(&Event::Destroy(10)));
    }

    #[test]
    fn uploaded_bitmap_reports_foreign_and_released_source_keys() {
        let mut first_backend = RecordingBackend::default();
        let foreign = {
            let mut first = UploadedSourceStore::new(&mut first_backend);
            first.upload_bitmap(&surface(1)).unwrap()
        };
        let mut second_backend = RecordingBackend::default();
        let mut second = UploadedSourceStore::new(&mut second_backend);
        assert_eq!(
            foreign.blit(&mut second, 99, one_pixel_blit()),
            Err(SourceBlitError::Access(SourceAccessError::ForeignKey(
                foreign.source_key()
            )))
        );
        assert!(second.backend().events.is_empty());

        let released = second.upload_bitmap(&surface(2)).unwrap();
        assert_eq!(second.release(released.source_key()), Ok(()));
        assert_eq!(
            released.blit(&mut second, 99, one_pixel_blit()),
            Err(SourceBlitError::Access(SourceAccessError::ReleasedKey(
                released.source_key()
            )))
        );
        assert_eq!(
            second.backend().events,
            [Event::Create(10), Event::Destroy(10)]
        );
    }

    #[test]
    fn committed_sources_resolve_and_drop_in_reverse_creation_order() {
        let mut backend = RecordingBackend::default();
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let first = store.upload(&surface(1)).unwrap();
            let second = store.upload(&surface(2)).unwrap();
            assert_eq!((first.slot(), second.slot()), (0, 1));
            assert_eq!(store.resolve(first), Ok(10));
            assert_eq!(store.resolve(second), Ok(11));
            assert_eq!(store.uploaded_source_count(), 2);
        }
        assert_eq!(
            backend.events,
            [
                Event::Create(10),
                Event::Create(11),
                Event::Destroy(11),
                Event::Destroy(10),
            ]
        );
    }

    #[test]
    fn uncommitted_failure_rolls_back_only_the_transaction() {
        let mut backend = RecordingBackend::default();
        let base;
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            base = store.upload(&surface(1)).unwrap();
            store.backend.fail_create_at = Some(2);
            let first_rolled_back;
            {
                let mut transaction = store.transaction();
                first_rolled_back = transaction.upload(&surface(2)).unwrap();
                assert!(matches!(
                    transaction.upload(&surface(3)),
                    Err(SourceUploadError::Backend(BackendError::Create(2)))
                ));
            }
            assert_eq!(store.resolve(base), Ok(10));
            assert_eq!(
                store.resolve(first_rolled_back),
                Err(SourceAccessError::ReleasedKey(first_rolled_back))
            );
            assert_eq!(store.uploaded_source_count(), 1);
        }
        assert_eq!(
            backend.events,
            [
                Event::Create(10),
                Event::Create(11),
                Event::Destroy(11),
                Event::Destroy(10),
            ]
        );
    }

    #[test]
    fn caught_upload_failure_can_continue_and_commit_a_later_source() {
        let mut backend = RecordingBackend {
            fail_create_at: Some(1),
            ..RecordingBackend::default()
        };
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            let (first, third) = {
                let mut transaction = store.transaction();
                let first = transaction.upload(&surface(1)).unwrap();
                assert!(matches!(
                    transaction.upload(&surface(2)),
                    Err(SourceUploadError::Backend(BackendError::Create(1)))
                ));
                let third = transaction.upload(&surface(3)).unwrap();
                transaction.commit();
                (first, third)
            };
            assert_eq!(store.resolve(first), Ok(10));
            assert_eq!(store.resolve(third), Ok(12));
            assert_eq!(store.uploaded_source_count(), 2);
        }
        assert_eq!(
            backend.events,
            [
                Event::Create(10),
                Event::Create(12),
                Event::Destroy(12),
                Event::Destroy(10),
            ]
        );
    }

    #[test]
    fn explicit_release_clears_before_destroy_and_store_drop_skips_it() {
        let mut backend = RecordingBackend::default();
        let key;
        {
            let mut store = UploadedSourceStore::new(&mut backend);
            key = store.upload(&surface(1)).unwrap();
            assert_eq!(store.release(key), Ok(()));
            assert_eq!(store.resolve(key), Err(SourceAccessError::ReleasedKey(key)));
            assert_eq!(store.release(key), Err(SourceAccessError::ReleasedKey(key)));
            assert_eq!(store.uploaded_source_count(), 0);
        }
        assert_eq!(backend.events, [Event::Create(10), Event::Destroy(10)]);
    }

    #[test]
    fn unwind_rolls_back_prior_transaction_sources_without_touching_base() {
        let mut backend = RecordingBackend::default();
        let mut store = UploadedSourceStore::new(&mut backend);
        let base = store.upload(&surface(1)).unwrap();
        store.backend.panic_create_at = Some(2);
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut transaction = store.transaction();
            transaction.upload(&surface(2)).unwrap();
            transaction.upload(&surface(3)).unwrap();
        }));
        assert!(unwind.is_err());
        assert_eq!(store.resolve(base), Ok(10));
        assert_eq!(store.uploaded_source_count(), 1);
        assert_eq!(store.backend.events.last(), Some(&Event::Destroy(11)));
    }

    #[test]
    fn foreign_unknown_released_and_backend_blit_errors_are_typed() {
        let mut first_backend = RecordingBackend::default();
        let mut second_backend = RecordingBackend::default();
        let foreign = {
            let mut first = UploadedSourceStore::new(&mut first_backend);
            first.upload(&surface(1)).unwrap()
        };
        let mut second = UploadedSourceStore::new(&mut second_backend);
        assert_eq!(
            second.resolve(foreign),
            Err(SourceAccessError::ForeignKey(foreign))
        );

        let unknown = SourceKey {
            store_id: second.store_id,
            slot: NonZeroUsize::new(9).unwrap(),
        };
        assert_eq!(
            second.resolve(unknown),
            Err(SourceAccessError::UnknownKey(unknown))
        );

        let released;
        {
            let mut transaction = second.transaction();
            released = transaction.upload(&surface(2)).unwrap();
        }
        assert_eq!(
            second.resolve(released),
            Err(SourceAccessError::ReleasedKey(released))
        );

        let live = second.upload(&surface(3)).unwrap();
        second.backend.fail_blit = true;
        let plan = crate::blit::plan_legacy_blit(
            crate::blit::RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            crate::blit::RectI16 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            1,
            1,
            None,
        );
        let crate::blit::BlitDecision::Blit(plan) = plan else {
            unreachable!()
        };
        assert_eq!(
            second.blit(live, 99, plan),
            Err(SourceBlitError::Backend(BackendError::Blit))
        );
        assert_eq!(
            second.blit(foreign, 99, plan),
            Err(SourceBlitError::Access(SourceAccessError::ForeignKey(
                foreign
            )))
        );
    }
}
