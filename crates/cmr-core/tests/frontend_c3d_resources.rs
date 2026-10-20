use std::{
    cell::RefCell,
    collections::BTreeMap,
    convert::Infallible,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use cmr_assets::{
    AssetFile, AssetProfile, AssetStore, ReviewedProfile, StorageEncoding, import_source,
};
use cmr_core::{
    c3d_topology::C3dObjectTopologyError,
    championship_assets::{CHAMPIONSHIP_CARS, MANUFACTURER_LOGOS, load_championship_assets},
    championship_car_frame_plan::{ChampionshipCarActorParent, plan_championship_car_frame},
    championship_car_transform::{ChampionshipCarInput, ChampionshipCarState},
    championship_setup::CanonicalCar,
    championship_setup_visual::PixelPoint,
    display::DisplayExtent,
    frontend_c3d_resources::{
        CHAMPIONSHIP_C3D_ENTRY_RESOURCE_SNAPSHOTS, CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN,
        FrontendC3dCarMaterializationError, FrontendC3dPreparationError,
        bind_frontend_c3d_resources, prepare_frontend_c3d_resources,
    },
    root_menu_assets::VisualVariant,
    root_menu_c3d_assets::{RootMenuC3dSlot, load_root_menu_c3d},
    root_menu_c3d_resources::{
        C3dEntryMaterialPolicy, C3dPreparedResourceAccessError, C3dResourceAllocationPolicy,
        C3dSourceIdentity, C3dSurfaceLayoutGates, FrontendC3dOwner,
    },
    root_menu_c3d_runtime::C3dResourceScaledMaterializationError,
};
use cmr_formats::{
    bitmap::BitmapLimits,
    c3d::{C3D_FORMAT_VERSION, C3D_MAGIC, C3dLimits},
    stored_asset::DecodeLimits,
};
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const REVIEWED_PROFILE: &[u8] =
    include_bytes!("../../../profiles/installed-eu-derived-assets-v2.json");
const SHARED_LABEL: [u8; 12] = *b"SHARED-CAR!!";
const ROOT_ENTRY_SNAPSHOTS: [u32; 6] = [55, 63, 63, 69, 73, 79];

#[test]
#[ignore = "requires a user-owned import selected with CMR_RUNTIME_DATA"]
fn reviewed_import_preserves_the_fresh_frontend_resource_snapshots() {
    let root = PathBuf::from(std::env::var_os("CMR_RUNTIME_DATA").expect("set CMR_RUNTIME_DATA"));
    let reviewed = ReviewedProfile::from_json(REVIEWED_PROFILE).unwrap();
    let store = AssetStore::open(
        &root,
        &reviewed,
        DecodeLimits {
            max_stored_bytes: 16 * 1024 * 1024,
            max_payload_bytes: 32 * 1024 * 1024,
        },
    )
    .unwrap();
    let (championship, root) = load_inputs(&store);
    let prepared =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();

    assert_eq!(
        prepared
            .cars()
            .map(|(_, object)| object.policy().entry_resource_count)
            .collect::<Vec<_>>(),
        CHAMPIONSHIP_C3D_ENTRY_RESOURCE_SNAPSHOTS
    );
    assert_eq!(
        prepared
            .root_slots()
            .map(|(_, object)| object.unwrap().preparation_policy().entry_resource_count)
            .collect::<Vec<_>>(),
        ROOT_ENTRY_SNAPSHOTS
    );
    assert_eq!(prepared.root_diagnostics().len(), 0);
    assert_eq!(prepared.resource_len(), 80);
    assert_eq!(
        prepared
            .cars()
            .flat_map(|(_, object)| object.ordinal_resources().iter().map(|id| id.index()))
            .collect::<Vec<_>>(),
        (0..CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN).collect::<Vec<_>>()
    );
    let expected_root_ids: [&[usize]; 6] = [
        &[55, 56, 57, 58, 59, 60, 61, 62],
        &[55, 56, 58, 57, 59, 61, 62],
        &[63, 64, 65, 66, 67, 68],
        &[69, 70, 71, 72],
        &[73, 74, 75, 76, 77, 78],
        &[79],
    ];
    for ((slot, object), expected_ids) in prepared.root_slots().zip(expected_root_ids) {
        let object = object.unwrap();
        assert_eq!(object.slot(), slot);
        assert_eq!(
            object
                .ordinal_resources()
                .iter()
                .map(|id| id.index())
                .collect::<Vec<_>>(),
            expected_ids
        );
    }

    const Q16_ONE: i32 = 0x1_0000;
    const IDENTITY_BASIS: [i32; 9] = [Q16_ONE, 0, 0, 0, Q16_ONE, 0, 0, 0, Q16_ONE];
    const EXTENT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };
    const ORIGIN: PixelPoint = PixelPoint { x: 100, y: 100 };
    const INPUT: ChampionshipCarInput = ChampionshipCarInput {
        f9: false,
        f10: false,
    };
    const HIERARCHY_COUNTS: [(usize, usize, usize, usize); 14] = [
        (17, 12, 15, 29),
        (17, 12, 15, 23),
        (17, 12, 15, 24),
        (19, 14, 17, 28),
        (17, 12, 15, 25),
        (17, 12, 15, 24),
        (17, 12, 15, 26),
        (17, 12, 15, 26),
        (18, 13, 16, 27),
        (17, 12, 15, 25),
        (17, 12, 15, 24),
        (16, 11, 14, 24),
        (19, 17, 17, 25),
        (18, 13, 16, 33),
    ];
    for (car_index, car) in CanonicalCar::ALL.into_iter().enumerate() {
        let topology = prepared.car_topology(&championship, car).unwrap();
        let runtime = prepared.materialize_car(car, &topology).unwrap();
        let mut state = ChampionshipCarState::new();
        let first = plan_championship_car_frame(
            &topology,
            &runtime,
            &mut state,
            car,
            INPUT,
            IDENTITY_BASIS,
            ORIGIN,
            EXTENT,
        )
        .unwrap();
        assert!(!first.selected_parent_visible());
        assert!(first.draws().is_empty());
        let second = plan_championship_car_frame(
            &topology,
            &runtime,
            &mut state,
            car,
            INPUT,
            IDENTITY_BASIS,
            ORIGIN,
            EXTENT,
        )
        .unwrap();
        assert!(second.selected_parent_visible());
        assert_eq!(
            (
                second.actors().len(),
                topology.groups().len(),
                second
                    .actors()
                    .iter()
                    .filter(|actor| !actor.levels().is_empty())
                    .count(),
                second.draws().len(),
            ),
            HIERARCHY_COUNTS[car_index]
        );
        assert_eq!(
            second.actors()[0].parent(),
            ChampionshipCarActorParent::PreviewRoot
        );
        assert!(second.actors()[0].levels().is_empty());
        for actor in &second.actors()[1..=4] {
            assert_eq!(actor.parent(), ChampionshipCarActorParent::Actor(0));
            assert_eq!(actor.levels().len(), 1);
        }
        assert_eq!(
            second.actors()[5].parent(),
            ChampionshipCarActorParent::Actor(0)
        );
        assert!(second.actors()[5].levels().is_empty());
        for actor in &second.actors()[6..] {
            assert_eq!(actor.parent(), ChampionshipCarActorParent::Actor(5));
            assert_eq!(actor.levels().len(), 1);
        }
        assert!(
            second
                .actors()
                .iter()
                .flat_map(|actor| actor.levels())
                .all(|level| level.flags() == 0)
        );
        assert!(!second.draws().is_empty());
        assert!(second.draws().iter().all(|draw| {
            draw.command().vertex_type() == 2
                && draw.command().primitive_code() == 4
                && draw.command().draw_flags() == 8
        }));
    }
}

#[test]
fn cars_preserve_exact_order_snapshots_mode8_policy_and_owned_provenance() {
    let fixture = Fixture::new(&complete_files(RootShape::SyntheticAliasAndGrowth));
    let (championship, root) = load_inputs(&fixture.store);
    let prepared =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();

    assert_eq!(prepared.cars().len(), CanonicalCar::ALL.len());
    for (index, (car, object)) in prepared.cars().enumerate() {
        let expected_resource_count = if car == CanonicalCar::M4r { 3 } else { 4 };
        assert_eq!(car, CanonicalCar::ALL[index]);
        assert_eq!(object.ordinal_resources().len(), expected_resource_count);
        assert_eq!(
            object.source(),
            &C3dSourceIdentity {
                owner: FrontendC3dOwner::ChampionshipCar(car),
                path: CHAMPIONSHIP_CARS[index].object_path.into(),
            }
        );
        assert_eq!(
            object.policy(),
            cmr_core::root_menu_c3d_resources::C3dLoadPolicy {
                mode: 8,
                caller_option: 1,
                caller_flags: 0x18,
                surface_constructor_flags: 0x15,
                entry_resource_count: CHAMPIONSHIP_C3D_ENTRY_RESOURCE_SNAPSHOTS[index],
                allocation: C3dResourceAllocationPolicy::AppendEveryResource,
                surface_layout: C3dSurfaceLayoutGates {
                    quarter_if_large: false,
                    force_quarter: false,
                    half_if_large: false,
                    expand_width_dominant: true,
                },
                material: C3dEntryMaterialPolicy::FixedUniformWord(0x9696_9696),
            }
        );
        assert!(object.policy().unknown_render_flag_08());
    }
}

#[test]
fn mode8_appends_exact_duplicates_and_builds_the_exact_55_resource_prefix() {
    let fixture = Fixture::new(&complete_files(RootShape::None));
    let (championship, root) = load_inputs(&fixture.store);
    let prepared =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();

    assert_eq!(
        prepared.resource_len(),
        CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN
    );
    let ids = prepared
        .cars()
        .flat_map(|(_, object)| object.ordinal_resources().iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(
        ids.iter().map(|id| id.index()).collect::<Vec<_>>(),
        (0..CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN).collect::<Vec<_>>()
    );
    assert!(
        prepared
            .resources()
            .take(CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN)
            .all(|(_, resource)| resource.source_label() == &SHARED_LABEL)
    );
}

#[test]
fn roots_start_at_55_in_the_same_arena_and_retain_mode2_aliasing_and_snapshots() {
    let fixture = Fixture::new(&complete_files(RootShape::SyntheticAliasAndGrowth));
    let (championship, root) = load_inputs(&fixture.store);
    let prepared =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let first_car_id = prepared.car(CanonicalCar::Subaru).ordinal_resources()[0];

    assert_eq!(prepared.root_slots().len(), RootMenuC3dSlot::ALL.len());
    assert_eq!(prepared.loaded_root_len(), RootMenuC3dSlot::ALL.len());
    assert_eq!(prepared.root_diagnostics().len(), 0);
    for (index, slot) in RootMenuC3dSlot::ALL.into_iter().enumerate() {
        let object = prepared.root(slot).unwrap();
        assert_eq!(object.slot(), slot);
        assert_eq!(
            object.source(),
            &C3dSourceIdentity {
                owner: FrontendC3dOwner::RootMenu(slot),
                path: slot.path(VisualVariant::Normal).into(),
            }
        );
        assert_eq!(object.preparation_policy().mode, 2);
        assert_eq!(object.preparation_policy().caller_option, 0);
        assert_eq!(object.preparation_policy().caller_flags, 0x11);
        assert_eq!(object.preparation_policy().surface_constructor_flags, 0x35);
        assert_eq!(
            object.preparation_policy().entry_resource_count,
            ROOT_ENTRY_SNAPSHOTS[index]
        );
        assert_eq!(
            object.preparation_policy().allocation,
            C3dResourceAllocationPolicy::InternExact
        );
        assert_eq!(
            object.preparation_policy().material,
            C3dEntryMaterialPolicy::Mode2Ordinary
        );
        assert_eq!(
            object.preparation_policy().surface_layout,
            C3dSurfaceLayoutGates {
                quarter_if_large: false,
                force_quarter: false,
                half_if_large: false,
                expand_width_dominant: true,
            }
        );
        assert_eq!(
            object.ordinal_resources().first().map(|id| id.index()),
            Some(if index <= 1 {
                first_car_id.index()
            } else {
                ROOT_ENTRY_SNAPSHOTS[index] as usize
            })
        );
        let first_resource = prepared.resource(object.ordinal_resources()[0]).unwrap();
        assert_eq!(
            first_resource
                .origin()
                .preparation_policy()
                .entry_resource_count,
            if index <= 1 {
                0
            } else {
                ROOT_ENTRY_SNAPSHOTS[index]
            }
        );
    }

    let championship_root = prepared.root(RootMenuC3dSlot::Championship).unwrap();
    assert_eq!(championship_root.ordinal_resources()[0], first_car_id);
    assert_eq!(championship_root.ordinal_resources()[1].index(), 55);
    assert_eq!(prepared.resource_len(), 85);
    assert_eq!(prepared.root_variant(), VisualVariant::Normal);
}

#[test]
fn one_session_owner_keeps_ids_stable_and_independent_sessions_reject_them() {
    let fixture = Fixture::new(&complete_files(RootShape::SyntheticAliasAndGrowth));
    let (championship, root) = load_inputs(&fixture.store);
    let first =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let second =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let id = first.car(CanonicalCar::Celica).ordinal_resources()[0];
    drop(championship);
    drop(root);

    assert_eq!(first.car(CanonicalCar::Celica).ordinal_resources()[0], id);
    assert_eq!(
        first.resource(id).unwrap().origin().source().owner,
        FrontendC3dOwner::ChampionshipCar(CanonicalCar::Celica)
    );
    assert_ne!(first.provenance(), second.provenance());
    assert_eq!(
        second.resource(id),
        Err(C3dPreparedResourceAccessError::Foreign)
    );
}

#[test]
fn car_topology_rejects_substituted_source_and_cross_car_materialization() {
    let files = complete_files(RootShape::None);
    let fixture = Fixture::new(&files);
    let (championship, root) = load_inputs(&fixture.store);
    let prepared =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();

    let subaru = prepared
        .car_topology(&championship, CanonicalCar::Subaru)
        .unwrap();
    let subaru_runtime = prepared
        .materialize_car(CanonicalCar::Subaru, &subaru)
        .unwrap();
    assert_eq!(
        subaru_runtime.policy(),
        prepared.car(CanonicalCar::Subaru).policy()
    );
    assert_eq!(
        prepared.materialize_car(CanonicalCar::Ibiza, &subaru),
        Err(FrontendC3dCarMaterializationError::ObjectMismatch {
            car: CanonicalCar::Ibiza,
        })
    );

    let second =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let second_subaru = second
        .car_topology(&championship, CanonicalCar::Subaru)
        .unwrap();
    assert!(matches!(
        prepared.materialize_car(CanonicalCar::Subaru, &second_subaru),
        Err(FrontendC3dCarMaterializationError::Materialization {
            car: CanonicalCar::Subaru,
            ref path,
            source: C3dResourceScaledMaterializationError::ForeignTopology { .. },
        }) if path.as_ref() == CHAMPIONSHIP_CARS[0].object_path
    ));

    let mut substituted_files = files;
    let substituted = substituted_files
        .get_mut(CHAMPIONSHIP_CARS[0].object_path)
        .unwrap();
    *substituted.last_mut().unwrap() ^= 1;
    let substituted_fixture = Fixture::new(&substituted_files);
    let (substituted_championship, _) = load_inputs(&substituted_fixture.store);
    assert!(matches!(
        prepared.car_topology(&substituted_championship, CanonicalCar::Subaru),
        Err(C3dObjectTopologyError::SourceMismatch { .. })
    ));
}

#[test]
fn frontend_catalog_maps_mixed_owners_once_and_preserves_arena_and_teardown() {
    let fixture = Fixture::new(&complete_files(RootShape::SyntheticAliasAndGrowth));
    let (championship, root) = load_inputs(&fixture.store);
    let prepared =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let foreign =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();
    let mut mapped = Vec::new();
    let catalog = bind_frontend_c3d_resources(&prepared, |id, resource| {
        mapped.push((
            id.index(),
            resource.origin().source().owner,
            resource.origin().preparation_policy().surface_layout,
        ));
        Ok::<_, Infallible>(id.index())
    })
    .unwrap();

    assert_eq!(catalog.provenance(), prepared.provenance());
    assert_eq!(catalog.len(), prepared.resource_len());
    assert_eq!(mapped.len(), prepared.resource_len());
    assert!(
        mapped
            .iter()
            .enumerate()
            .all(|(index, mapped)| mapped.0 == index)
    );
    assert!(
        mapped[..CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN]
            .iter()
            .all(|(_, owner, _)| matches!(owner, FrontendC3dOwner::ChampionshipCar(_)))
    );
    assert!(
        mapped[CHAMPIONSHIP_C3D_RESOURCE_PREFIX_LEN..]
            .iter()
            .all(|(_, owner, _)| matches!(owner, FrontendC3dOwner::RootMenu(_)))
    );
    assert_eq!(
        mapped[0],
        (
            0,
            FrontendC3dOwner::ChampionshipCar(CanonicalCar::Subaru),
            C3dSurfaceLayoutGates {
                expand_width_dominant: true,
                ..C3dSurfaceLayoutGates::default()
            },
        )
    );
    assert!(matches!(mapped[55].1, FrontendC3dOwner::RootMenu(_)));
    let aliased_from_car = prepared
        .root(RootMenuC3dSlot::Championship)
        .unwrap()
        .ordinal_resources()[0];
    assert_eq!(aliased_from_car.index(), 0);
    assert_eq!(*catalog.binding(aliased_from_car).unwrap(), 0);
    assert_eq!(
        prepared
            .resource(aliased_from_car)
            .unwrap()
            .origin()
            .source()
            .owner,
        FrontendC3dOwner::ChampionshipCar(CanonicalCar::Subaru)
    );
    for (id, _) in prepared.resources() {
        assert_eq!(*catalog.binding(id).unwrap(), id.index());
    }
    let foreign_id = foreign.resources().next().unwrap().0;
    assert_eq!(
        catalog.binding(foreign_id),
        Err(C3dPreparedResourceAccessError::Foreign)
    );

    #[derive(Debug)]
    struct DropBinding {
        index: usize,
        drops: Rc<RefCell<Vec<usize>>>,
    }
    impl Drop for DropBinding {
        fn drop(&mut self) {
            self.drops.borrow_mut().push(self.index);
        }
    }

    let rollback_drops = Rc::new(RefCell::new(Vec::new()));
    let error = bind_frontend_c3d_resources(&prepared, |id, resource| {
        if id.index() == 3 {
            Err("bind")
        } else {
            assert!(matches!(
                resource.origin().source().owner,
                FrontendC3dOwner::ChampionshipCar(_)
            ));
            Ok(DropBinding {
                index: id.index(),
                drops: Rc::clone(&rollback_drops),
            })
        }
    })
    .unwrap_err();
    assert_eq!(error.resource_id.index(), 3);
    assert_eq!(
        error.origin.source().owner,
        FrontendC3dOwner::ChampionshipCar(CanonicalCar::Subaru)
    );
    assert_eq!(error.origin.source_resource_ordinal(), 3);
    assert_eq!(error.source, "bind");
    assert_eq!(rollback_drops.borrow().as_slice(), [2, 1, 0]);

    let committed_drops = Rc::new(RefCell::new(Vec::new()));
    let committed = bind_frontend_c3d_resources(&prepared, |id, _| {
        Ok::<_, Infallible>(DropBinding {
            index: id.index(),
            drops: Rc::clone(&committed_drops),
        })
    })
    .unwrap();
    drop(committed);
    assert_eq!(
        committed_drops.borrow().as_slice(),
        (0..prepared.resource_len()).rev().collect::<Vec<_>>()
    );
}

#[test]
fn root_failure_rolls_back_its_additions_leaves_a_hole_and_continues() {
    let fixture = Fixture::new(&complete_files(RootShape::FailureThenSuccess));
    let (championship, root) = load_inputs(&fixture.store);
    let prepared =
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()).unwrap();

    assert!(prepared.root(RootMenuC3dSlot::Championship).is_none());
    let diagnostic = prepared.root_diagnostics().next().unwrap();
    assert_eq!(diagnostic.slot(), RootMenuC3dSlot::Championship);
    assert_eq!(prepared.root_diagnostics().len(), 1);
    let rally = prepared.root(RootMenuC3dSlot::Rally).unwrap();
    assert_eq!(rally.ordinal_resources()[0].index(), 55);
    assert_eq!(
        prepared
            .resource(rally.ordinal_resources()[0])
            .unwrap()
            .origin()
            .preparation_policy()
            .entry_resource_count,
        55
    );
    assert_eq!(prepared.resource_len(), 56);
}

#[test]
fn required_car_shape_and_decode_failures_abort_with_car_context() {
    let mut wrong_count = complete_files(RootShape::None);
    wrong_count.insert(
        CHAMPIONSHIP_CARS[0].object_path,
        c3d_with_blobs(&car_blobs(0, 3)),
    );
    let fixture = Fixture::new(&wrong_count);
    let (championship, root) = load_inputs(&fixture.store);
    assert!(matches!(
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()),
        Err(FrontendC3dPreparationError::CarResourceCount {
            car: CanonicalCar::Subaru,
            expected: 4,
            actual: 3,
            ..
        })
    ));

    let mut malformed = complete_files(RootShape::None);
    let mut blobs = car_blobs(0, 4);
    blobs[2].1 = b"opaque".to_vec();
    malformed.insert(CHAMPIONSHIP_CARS[0].object_path, c3d_with_blobs(&blobs));
    let fixture = Fixture::new(&malformed);
    let (championship, root) = load_inputs(&fixture.store);
    assert!(matches!(
        prepare_frontend_c3d_resources(&championship, &root, BitmapLimits::default()),
        Err(FrontendC3dPreparationError::CarResource {
            car: CanonicalCar::Subaru,
            ..
        })
    ));
}

#[derive(Clone, Copy)]
enum RootShape {
    None,
    /// Synthetic alias/growth exercise. It intentionally ends at 85 resources
    /// and is not a cardinality model of the reviewed import, which ends at 80.
    SyntheticAliasAndGrowth,
    FailureThenSuccess,
}

fn load_inputs(
    store: &AssetStore,
) -> (
    cmr_core::championship_assets::ChampionshipAssets,
    cmr_core::root_menu_c3d_assets::RootMenuC3dSet,
) {
    (
        load_championship_assets(store, C3dLimits::default(), BitmapLimits::default()).unwrap(),
        load_root_menu_c3d(store, VisualVariant::Normal, C3dLimits::default()),
    )
}

fn complete_files(root_shape: RootShape) -> BTreeMap<&'static str, Vec<u8>> {
    let mut files = BTreeMap::new();
    for (index, descriptor) in CHAMPIONSHIP_CARS.into_iter().enumerate() {
        let count = if descriptor.car == CanonicalCar::M4r {
            3
        } else {
            4
        };
        files.insert(
            descriptor.object_path,
            c3d_with_blobs(&car_blobs(index, count)),
        );
        files.insert(descriptor.palette_path, synthetic_palette(index as u8));
    }
    for (index, descriptor) in MANUFACTURER_LOGOS.into_iter().enumerate() {
        files.insert(descriptor.path, indexed_bmp(100 + index as u8, 0));
    }

    match root_shape {
        RootShape::None => {}
        RootShape::SyntheticAliasAndGrowth => {
            let counts = [9, 1, 6, 4, 6, 6];
            for (slot_index, slot) in RootMenuC3dSlot::ALL.into_iter().enumerate() {
                let mut blobs = Vec::new();
                for ordinal in 0..counts[slot_index] {
                    if (slot_index == 0 && ordinal == 0) || slot_index == 1 {
                        blobs.push((SHARED_LABEL, indexed_bmp(7, 200)));
                    } else {
                        blobs.push((
                            root_label(slot_index as u8, ordinal as u8),
                            indexed_bmp(40 + slot_index as u8 * 8 + ordinal as u8, 10),
                        ));
                    }
                }
                files.insert(slot.path(VisualVariant::Normal), c3d_with_blobs(&blobs));
            }
        }
        RootShape::FailureThenSuccess => {
            files.insert(
                RootMenuC3dSlot::Championship.path(VisualVariant::Normal),
                c3d_with_blobs(&[
                    (root_label(0, 0), indexed_bmp(80, 0)),
                    (root_label(0, 1), b"opaque".to_vec()),
                ]),
            );
            files.insert(
                RootMenuC3dSlot::Rally.path(VisualVariant::Normal),
                c3d_with_blobs(&[(root_label(1, 0), indexed_bmp(81, 0))]),
            );
        }
    }
    files
}

fn car_blobs(_car: usize, count: usize) -> Vec<([u8; 12], Vec<u8>)> {
    (0..count)
        .map(|_| (SHARED_LABEL, indexed_bmp(7, 0)))
        .collect()
}

fn root_label(slot: u8, ordinal: u8) -> [u8; 12] {
    let mut label = *b"ROOT-00-000!";
    label[5] = b'0' + slot;
    label[7] = b'0' + ordinal / 10;
    label[8] = b'0' + ordinal % 10;
    label
}

fn c3d_with_blobs(blobs: &[([u8; 12], Vec<u8>)]) -> Vec<u8> {
    let blob_start = 32usize;
    let total_len = blob_start + blobs.iter().map(|(_, blob)| 16 + blob.len()).sum::<usize>();
    let mut bytes = vec![0; total_len];
    bytes[0..4].copy_from_slice(&C3D_MAGIC);
    bytes[4..8].copy_from_slice(&C3D_FORMAT_VERSION.to_le_bytes());
    bytes[16..18].copy_from_slice(&(blobs.len() as u16).to_le_bytes());
    for (index, offset) in [32u32, 32, 32].into_iter().enumerate() {
        let start = 20 + index * 4;
        bytes[start..start + 4].copy_from_slice(&offset.to_le_bytes());
    }
    let mut offset = blob_start;
    for (label, blob) in blobs {
        bytes[offset..offset + 12].copy_from_slice(label);
        bytes[offset + 12..offset + 16].copy_from_slice(&(blob.len() as u32).to_le_bytes());
        bytes[offset + 16..offset + 16 + blob.len()].copy_from_slice(blob);
        offset += 16 + blob.len();
    }
    bytes
}

fn indexed_bmp(pixel: u8, palette_blue: u8) -> Vec<u8> {
    let pixel_offset = 14 + 40 + 256 * 4;
    let file_len = pixel_offset + 4;
    let mut bytes = vec![0; file_len];
    bytes[0..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&(file_len as u32).to_le_bytes());
    bytes[10..14].copy_from_slice(&(pixel_offset as u32).to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&1i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&1i32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&8u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&4u32.to_le_bytes());
    bytes[46..50].copy_from_slice(&256u32.to_le_bytes());
    bytes[54] = palette_blue;
    bytes[pixel_offset] = pixel;
    bytes
}

fn synthetic_palette(marker: u8) -> Vec<u8> {
    let data_size = 4 + 256 * 4;
    let file_size = 12 + 8 + data_size;
    let mut input = vec![0; file_size];
    input[0..4].copy_from_slice(b"RIFF");
    input[4..8].copy_from_slice(&((file_size - 8) as u32).to_le_bytes());
    input[8..12].copy_from_slice(b"PAL ");
    input[12..16].copy_from_slice(b"data");
    input[16..20].copy_from_slice(&(data_size as u32).to_le_bytes());
    input[20..22].copy_from_slice(&0x0300u16.to_le_bytes());
    input[22..24].copy_from_slice(&256u16.to_le_bytes());
    input[24] = marker;
    input
}

struct Fixture {
    _temporary: TestDirectory,
    store: AssetStore,
}

impl Fixture {
    fn new(files: &BTreeMap<&'static str, Vec<u8>>) -> Self {
        let temporary = TestDirectory::new();
        let source = temporary.0.join("source");
        fs::create_dir(&source).unwrap();
        let mut profile_files = Vec::with_capacity(files.len());
        for (path, bytes) in files {
            let destination = source.join(path);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(&destination, bytes).unwrap();
            profile_files.push(asset(path, bytes));
        }
        profile_files.sort_by(|left, right| left.path.cmp(&right.path));
        let profile = AssetProfile {
            schema_version: 1,
            id: "synthetic-frontend-c3d".to_owned(),
            roots: vec!["New_Fe".to_owned()],
            empty_directories: Vec::new(),
            sentinels: Vec::new(),
            files: profile_files,
        };
        let reviewed = ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
        let imported = temporary.0.join("runtime-data");
        import_source(&source, &imported, &reviewed).unwrap();
        let store = AssetStore::open(
            &imported,
            &reviewed,
            DecodeLimits {
                max_stored_bytes: 2 * 1024 * 1024,
                max_payload_bytes: 2 * 1024 * 1024,
            },
        )
        .unwrap();
        Self {
            _temporary: temporary,
            store,
        }
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("cmr-frontend-c3d-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn asset(path: &str, bytes: &[u8]) -> AssetFile {
    AssetFile {
        path: path.to_owned(),
        size: bytes.len() as u64,
        sha256: hex_sha256(bytes),
        encoding: StorageEncoding::Raw,
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(encoded, "{byte:02x}").unwrap();
    }
    encoded
}
