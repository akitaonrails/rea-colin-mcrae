//! Concrete pure adapter joining the recovered static narrow phases to the
//! official-EU normal collision response. A kernel value represents one
//! collision pass and retains the sequential car/cache state between records.

use std::collections::VecDeque;
use std::convert::Infallible;

use crate::{
    fixed_point::{Fixed16, Vec3},
    static_collision_response::{
        DamageResult, ResponseBasis, STATIC_RESPONSE_CONTACT_ID, StaticCollisionResponseInput,
        StaticCollisionResponseOutcome, StaticCollisionResponseState, StaticDamageEvaluation,
        StaticDamageOwner, StaticDamageRequest, apply_static_collision_response,
        evaluate_static_damage, static_damage_emits_contact_effect,
    },
    static_collision_response_input::{
        StaticResponseInputProducerError, StaticResponseInputProducerState,
        StaticResponseProducedInput, produce_static_response_input, static_response_canonical_axes,
    },
    static_world_collision::{
        AlternateStaticNarrowResult, StaticCarCollisionFrame, StaticCarNarrowInput,
        StaticCollisionContribution, StaticCollisionKernel, StaticCollisionRecord,
        StaticCollisionResolution, StaticCollisionRunningState, StaticNarrowObb,
        StaticNarrowResult, apply_alternate_static_narrow_phase, resolve_static_obb_narrow_phase,
    },
};

/// FIFO replay/test values for external queries and mutable calls kept outside
/// this pure adapter. This is not an honest live damage owner: live integration
/// needs a future resumable boundary that pauses at each damage mutation and
/// resumes with its effect handle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticKernelExternalInputs {
    pub type_one_effect_gate_blocked: bool,
    pub damage_result: DamageResult,
    pub capture_debug_material: bool,
}

/// Car/cache fields not carried by [`StaticCarCollisionFrame`]. Values here
/// are recovered inputs, not defaults inferred by the adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticKernelContext {
    pub projected_corners: [Vec3; 4],
    pub projection_axis: Vec3,
    pub projection_perpendicular: Vec3,
    pub projection_axis_radius: Fixed16,
    pub projection_perpendicular_radius: Fixed16,
    pub y_gate_552f60: Fixed16,
    pub y_gate_552f80: Fixed16,
    pub velocity_d4: Vec3,
    pub alternate_axis_324: Vec3,
    pub projected_axis_270: Vec3,
    pub response_basis_2a0: ResponseBasis,
    pub out_of_control_680: bool,
    pub out_of_control_688: bool,
    pub car_type_738: i8,
    pub car_subtype_739: i8,
    pub contact_count_674: i32,
    pub velocity_normalizer_ec: Fixed16,
    pub fallback_axis_258: Vec3,
}

/// One admitted record and the exact path it took. Normal outcomes retain the
/// ordered external-call stand-ins emitted by the response contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaticKernelHit {
    Normal {
        collision_ordinal: u16,
        narrow: StaticNarrowResult,
        produced_input: StaticResponseProducedInput,
        response: Box<StaticCollisionResponseOutcome>,
    },
    Alternate {
        collision_ordinal: u16,
        narrow: AlternateStaticNarrowResult,
    },
}

/// Sequential state left after the records processed by one kernel value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticKernelState {
    pub position: Vec3,
    pub hull_points: [Vec3; 8],
    pub projected_corners: [Vec3; 4],
    pub velocity_d4: Vec3,
    pub out_of_control_680: bool,
    pub out_of_control_688: bool,
    pub alternate_radial_weights_304: [Fixed16; 8],
    pub alternate_point_flags_69c: [i32; 8],
}

/// Concrete adapter for `StaticCollisionKernel<StaticNarrowObb>`.
///
/// Construct one value per collision pass. The first `resolve` snapshots the
/// frame's center/hull; subsequent calls use the depenetrated cache produced by
/// earlier normal contacts, matching the native record order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoStaticDamageOwner;

impl StaticDamageOwner for NoStaticDamageOwner {
    type Error = Infallible;

    fn commit_contact(
        &mut self,
        _request: StaticDamageRequest,
    ) -> Result<DamageResult, Self::Error> {
        Ok(DamageResult::default())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StaticDamageProvider<D> {
    Replay,
    Live(D),
}

/// Kernel adapter with either the legacy FIFO replay seam or a synchronous
/// live mutable-damage owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveredStaticCollisionKernel<D = NoStaticDamageOwner> {
    context: StaticKernelContext,
    external_inputs: VecDeque<StaticKernelExternalInputs>,
    damage_provider: StaticDamageProvider<D>,
    state: Option<StaticKernelState>,
    hits: Vec<StaticKernelHit>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StaticKernelError<E = Infallible> {
    MissingExternalInput {
        collision_ordinal: u16,
        normal_hit_index: usize,
    },
    ResponseInputProducer {
        collision_ordinal: u16,
        source: StaticResponseInputProducerError,
    },
    /// Replay supplied a handle although the strict native threshold branch
    /// did not enter the external mutable damage sequence.
    ImpossibleEffectHandle {
        collision_ordinal: u16,
        effect_handle: i32,
    },
    DamageOwner {
        collision_ordinal: u16,
        source: E,
    },
}

impl RecoveredStaticCollisionKernel<NoStaticDamageOwner> {
    #[must_use]
    pub fn new(
        context: StaticKernelContext,
        external_inputs: impl IntoIterator<Item = StaticKernelExternalInputs>,
    ) -> Self {
        Self {
            context,
            external_inputs: external_inputs.into_iter().collect(),
            damage_provider: StaticDamageProvider::Replay,
            state: None,
            hits: Vec::new(),
        }
    }
}

impl<D> RecoveredStaticCollisionKernel<D> {
    /// Bind a live damage owner. Live resolution does not consume the replay
    /// FIFO: the owner supplies the synchronous type-one query and debug gate
    /// as well as lazily committing admitted damage.
    #[must_use]
    pub fn with_damage_owner(
        context: StaticKernelContext,
        external_inputs: impl IntoIterator<Item = StaticKernelExternalInputs>,
        damage_owner: D,
    ) -> Self {
        Self {
            context,
            external_inputs: external_inputs.into_iter().collect(),
            damage_provider: StaticDamageProvider::Live(damage_owner),
            state: None,
            hits: Vec::new(),
        }
    }

    #[must_use]
    pub fn state(&self) -> Option<&StaticKernelState> {
        self.state.as_ref()
    }

    #[must_use]
    pub fn hits(&self) -> &[StaticKernelHit] {
        &self.hits
    }

    #[must_use]
    pub fn damage_owner(&self) -> Option<&D> {
        match &self.damage_provider {
            StaticDamageProvider::Replay => None,
            StaticDamageProvider::Live(owner) => Some(owner),
        }
    }

    fn state_for_frame(&mut self, frame: &StaticCarCollisionFrame) -> &mut StaticKernelState {
        self.state.get_or_insert(StaticKernelState {
            position: frame.center,
            hull_points: frame.world_hull_points,
            projected_corners: self.context.projected_corners,
            velocity_d4: self.context.velocity_d4,
            out_of_control_680: self.context.out_of_control_680,
            out_of_control_688: self.context.out_of_control_688,
            alternate_radial_weights_304: [Fixed16::ZERO; 8],
            alternate_point_flags_69c: [0; 8],
        })
    }

    fn narrow_input(&mut self, frame: &StaticCarCollisionFrame) -> StaticCarNarrowInput {
        let context = self.context;
        let state = self.state_for_frame(frame);
        StaticCarNarrowInput {
            position: state.position,
            previous_center_xz: frame.previous_center_xz,
            hull_points: state.hull_points,
            projected_corners: state.projected_corners,
            projection_axis: context.projection_axis,
            projection_perpendicular: context.projection_perpendicular,
            projection_axis_radius: context.projection_axis_radius,
            projection_perpendicular_radius: context.projection_perpendicular_radius,
            y_gate_552f60: context.y_gate_552f60,
            y_gate_552f80: context.y_gate_552f80,
            velocity_d4: state.velocity_d4,
            alternate_axis_324: context.alternate_axis_324,
        }
    }
}

impl<D: StaticDamageOwner> RecoveredStaticCollisionKernel<D> {
    fn obtain_damage_result(
        &mut self,
        collision_ordinal: u16,
        evaluation: Option<StaticDamageEvaluation>,
        contact_3f0: Vec3,
        linear_correction_3cc: Vec3,
        replay_external: Option<StaticKernelExternalInputs>,
    ) -> Result<DamageResult, StaticKernelError<D::Error>> {
        let invoked = evaluation.is_some_and(|value| value.damage_sequence_invoked);
        if invoked {
            return match &mut self.damage_provider {
                StaticDamageProvider::Replay => Ok(replay_external
                    .expect("replay input was validated")
                    .damage_result),
                StaticDamageProvider::Live(owner) => owner
                    .commit_contact(StaticDamageRequest {
                        car_type_738: self.context.car_type_738,
                        car_subtype_739: self.context.car_subtype_739,
                        contact_3f0,
                        linear_correction_3cc,
                        severity: evaluation
                            .expect("an invoked damage sequence has an evaluation")
                            .severity,
                    })
                    .map_err(|source| StaticKernelError::DamageOwner {
                        collision_ordinal,
                        source,
                    }),
            };
        }

        Ok(match &self.damage_provider {
            // Preserve the old deterministic replay validation without
            // invoking a live owner below threshold or for repeated history.
            StaticDamageProvider::Replay => {
                replay_external
                    .expect("replay input was validated")
                    .damage_result
            }
            StaticDamageProvider::Live(_) => DamageResult::default(),
        })
    }
}

impl<D: StaticDamageOwner> StaticCollisionKernel<StaticNarrowObb>
    for RecoveredStaticCollisionKernel<D>
{
    type Error = StaticKernelError<D::Error>;

    fn resolve(
        &mut self,
        frame: &StaticCarCollisionFrame,
        record: &StaticCollisionRecord<StaticNarrowObb>,
        running: StaticCollisionRunningState,
    ) -> Result<StaticCollisionResolution, Self::Error> {
        let car = self.narrow_input(frame);
        if record.alternate_collision_path() {
            let Some(narrow) = apply_alternate_static_narrow_phase(car, record.geometry) else {
                return Ok(StaticCollisionResolution::Miss);
            };
            let state = self.state.as_mut().expect("narrow input initializes state");
            state.velocity_d4 = narrow.velocity_d4;
            state.alternate_radial_weights_304 = narrow.radial_weights_304;
            state.alternate_point_flags_69c = narrow.point_flags_69c;
            self.hits.push(StaticKernelHit::Alternate {
                collision_ordinal: record.collision_ordinal,
                narrow,
            });
            return Ok(StaticCollisionResolution::Alternate);
        }

        let narrow = resolve_static_obb_narrow_phase(car, record.geometry);
        if !narrow.hit {
            return Ok(StaticCollisionResolution::Miss);
        }
        let narrow_normal = narrow
            .contact_normal
            .expect("a normal-path hit publishes its direction");
        let contact_offset_world = narrow
            .contact_offset_world
            .expect("a normal-path hit publishes its contact offset");
        let context = self.context;
        let contact_3f0 = context
            .response_basis_2a0
            .world_to_body(contact_offset_world);
        let velocity_d4 = self
            .state
            .as_ref()
            .expect("narrow input initializes state")
            .velocity_d4;
        let canonical_axes = static_response_canonical_axes();
        let produced_input = produce_static_response_input(StaticResponseInputProducerState {
            existing_normal: narrow_normal,
            contact_3f0,
            contact_limits_2c: frame.angular_limit_2c,
            contact_count_674: context.contact_count_674,
            velocity_normalizer_ec: context.velocity_normalizer_ec,
            velocity_d4,
            basis_2a0: context.response_basis_2a0,
            canonical_axis_552f28: canonical_axes.axis_552f28,
            canonical_axis_552f34: canonical_axes.axis_552f34,
            fallback_axis_258: context.fallback_axis_258,
            fallback_axis_270: context.projected_axis_270,
        })
        .map_err(|source| StaticKernelError::ResponseInputProducer {
            collision_ordinal: record.collision_ordinal,
            source,
        })?;
        // Validate/consume the per-response external boundary before mutating
        // any sequential car/cache fields. A missing input is therefore atomic
        // apart from the documented lazy initialization performed to run the
        // narrow phase.
        let replay_external = match &self.damage_provider {
            StaticDamageProvider::Replay => Some(
                self.external_inputs.front().copied().ok_or(
                    StaticKernelError::MissingExternalInput {
                        collision_ordinal: record.collision_ordinal,
                        normal_hit_index: self
                            .hits
                            .iter()
                            .filter(|hit| matches!(hit, StaticKernelHit::Normal { .. }))
                            .count(),
                    },
                )?,
            ),
            StaticDamageProvider::Live(_) => None,
        };
        let (type_one_effect_gate_blocked, capture_debug_material) = match &mut self.damage_provider
        {
            StaticDamageProvider::Replay => {
                let external = replay_external.expect("replay input was validated");
                (
                    external.type_one_effect_gate_blocked,
                    external.capture_debug_material,
                )
            }
            StaticDamageProvider::Live(owner) => {
                let blocked = if context.car_type_738 == 1 {
                    owner.type_one_effect_gate_blocked().map_err(|source| {
                        StaticKernelError::DamageOwner {
                            collision_ordinal: record.collision_ordinal,
                            source,
                        }
                    })?
                } else {
                    false
                };
                (blocked, owner.capture_debug_material())
            }
        };
        let full_world_correction = produced_input
            .normal
            .scale(produced_input.normal.dot(velocity_d4).neg());
        let prospective_linear_correction = running.linear_impulse_3cc.add(full_world_correction);
        let previous_contact = running
            .history
            .contained_last_tick(STATIC_RESPONSE_CONTACT_ID);
        let damage_evaluation =
            (!previous_contact).then(|| evaluate_static_damage(prospective_linear_correction));
        let damage_result = self.obtain_damage_result(
            record.collision_ordinal,
            damage_evaluation,
            contact_3f0,
            prospective_linear_correction,
            replay_external,
        )?;
        if !previous_contact
            && damage_result.effect_handle != 0
            && !static_damage_emits_contact_effect(
                damage_evaluation.expect("new contact computes damage evaluation"),
                damage_result.effect_handle,
            )
        {
            return Err(StaticKernelError::ImpossibleEffectHandle {
                collision_ordinal: record.collision_ordinal,
                effect_handle: damage_result.effect_handle,
            });
        }
        if replay_external.is_some() {
            self.external_inputs.pop_front();
        }
        let state = self.state.as_mut().expect("narrow input initializes state");
        state.position = narrow.final_position;
        state.hull_points = narrow.final_hull_points;
        state.projected_corners = narrow.final_projected_corners;
        let response = apply_static_collision_response(
            StaticCollisionResponseState {
                velocity_d4: state.velocity_d4,
                projected_axis_270: context.projected_axis_270,
                basis_2a0: context.response_basis_2a0,
                position_c8: state.position,
                contact_3f0,
                contact_limits_2c: frame.angular_limit_2c,
                linear_correction_3cc: running.linear_impulse_3cc,
                angular_correction_3d8: running.angular_impulse_3d8,
                out_of_control_680: state.out_of_control_680,
                out_of_control_688: state.out_of_control_688,
                car_type_738: context.car_type_738,
                history: running.history,
            },
            StaticCollisionResponseInput {
                normal: produced_input.normal,
                primary_limit: produced_input.primary_limit,
                projected_limit: produced_input.projected_limit,
                raw_material_flags: record.raw_material_flags,
                contact_id: STATIC_RESPONSE_CONTACT_ID,
                type_one_effect_gate_blocked,
                damage_result,
                capture_debug_material,
            },
        );
        state.velocity_d4 = response.state.velocity_d4;
        state.out_of_control_680 = response.state.out_of_control_680;
        state.out_of_control_688 = response.state.out_of_control_688;
        let contribution = StaticCollisionContribution {
            contact_normal: produced_input.normal,
            linear_impulse_3cc: response.state.linear_correction_3cc,
            angular_impulse_3d8: response.state.angular_correction_3d8,
            local_contact_arm_3f0: response.state.contact_3f0,
            history: response.state.history,
        };
        self.hits.push(StaticKernelHit::Normal {
            collision_ordinal: record.collision_ordinal,
            narrow,
            produced_input,
            response: Box::new(response),
        });
        Ok(StaticCollisionResolution::Normal(contribution))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::static_collision_response::{
        StaticResponseEvent, evaluate_static_damage_length, static_damage_severity,
    };
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    const ONE: i32 = 0x1_0000;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct TestDamageError;

    #[derive(Debug)]
    struct RecordingDamageOwner {
        calls: Rc<RefCell<Vec<StaticDamageRequest>>>,
        handles: VecDeque<i32>,
        fail: bool,
    }

    impl RecordingDamageOwner {
        fn returning(handles: impl IntoIterator<Item = i32>) -> Self {
            Self {
                calls: Rc::default(),
                handles: handles.into_iter().collect(),
                fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                calls: Rc::default(),
                handles: VecDeque::new(),
                fail: true,
            }
        }
    }

    impl StaticDamageOwner for RecordingDamageOwner {
        type Error = TestDamageError;

        fn commit_contact(
            &mut self,
            request: StaticDamageRequest,
        ) -> Result<DamageResult, Self::Error> {
            self.calls.borrow_mut().push(request);
            if self.fail {
                return Err(TestDamageError);
            }
            Ok(DamageResult {
                effect_handle: self.handles.pop_front().unwrap_or_default(),
            })
        }
    }

    const fn q(raw: i32) -> Fixed16 {
        Fixed16::from_raw(raw)
    }

    const fn v(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(q(x), q(y), q(z))
    }

    fn identity() -> ResponseBasis {
        ResponseBasis {
            rows: [v(ONE, 0, 0), v(0, ONE, 0), v(0, 0, ONE)],
        }
    }

    fn projected_box(radius: i32) -> [Vec3; 4] {
        [
            v(-radius, 0, -radius),
            v(radius, 0, -radius),
            v(radius, 0, radius),
            v(-radius, 0, radius),
        ]
    }

    fn frame() -> StaticCarCollisionFrame {
        let corners = projected_box(ONE);
        let mut hull = [Vec3::ZERO; 8];
        hull[..4].copy_from_slice(&corners);
        hull[4..].copy_from_slice(&corners);
        StaticCarCollisionFrame {
            center: Vec3::ZERO,
            previous_center_xz: [q(-10 * ONE), Fixed16::ZERO],
            world_hull_points: hull,
            active_nodes: [None; 4],
            linear_impulse_3cc: Vec3::ZERO,
            angular_impulse_3d8: Vec3::ZERO,
            local_contact_arm_3f0: Vec3::ZERO,
            angular_limit_2c: v(10 * ONE, -ONE, 10 * ONE),
        }
    }

    fn context() -> StaticKernelContext {
        StaticKernelContext {
            projected_corners: projected_box(ONE),
            projection_axis: v(ONE, 0, 0),
            projection_perpendicular: v(0, 0, ONE),
            projection_axis_radius: q(ONE),
            projection_perpendicular_radius: q(ONE),
            y_gate_552f60: Fixed16::ZERO,
            y_gate_552f80: Fixed16::ZERO,
            velocity_d4: v(-ONE, 0, ONE),
            alternate_axis_324: v(ONE, 0, 0),
            projected_axis_270: v(ONE, 0, 0),
            response_basis_2a0: identity(),
            out_of_control_680: false,
            out_of_control_688: false,
            car_type_738: 0,
            car_subtype_739: 0,
            contact_count_674: 3,
            velocity_normalizer_ec: q(ONE),
            fallback_axis_258: v(0, 0, ONE),
        }
    }

    fn object(center_x: i32) -> StaticNarrowObb {
        StaticNarrowObb {
            center: v(center_x, 0, 0),
            projected_corners: [[-128, -128], [128, -128], [128, 128], [-128, 128]],
            radius: q(2 * ONE),
            max_y_shift9: 128,
            min_y_shift9: -128,
            axis_radius: q(ONE),
            perpendicular_radius: q(ONE),
            axis: v(ONE, 0, 0),
            perpendicular: v(0, 0, ONE),
        }
    }

    fn record(flags: u8, geometry: StaticNarrowObb) -> StaticCollisionRecord<StaticNarrowObb> {
        record_with_id(7, flags, geometry)
    }

    fn record_with_id(
        collision_ordinal: u16,
        flags: u8,
        geometry: StaticNarrowObb,
    ) -> StaticCollisionRecord<StaticNarrowObb> {
        StaticCollisionRecord {
            collision_ordinal,
            raw_material_flags: flags,
            geometry,
        }
    }

    fn running() -> StaticCollisionRunningState {
        StaticCollisionRunningState {
            linear_impulse_3cc: v(3 * ONE, 0, 0),
            angular_impulse_3d8: v(0, ONE, 0),
            local_contact_arm_3f0: v(9, 8, 7),
            history: Default::default(),
        }
    }

    #[test]
    fn normal_miss_does_not_change_kernel_state_or_publish_a_hit() {
        let mut kernel =
            RecoveredStaticCollisionKernel::new(context(), [StaticKernelExternalInputs::default()]);
        let result = kernel
            .resolve(&frame(), &record(5, object(20 * ONE)), running())
            .unwrap();
        assert_eq!(result, StaticCollisionResolution::Miss);
        assert!(kernel.hits().is_empty());
        assert_eq!(kernel.state().unwrap().position, Vec3::ZERO);
        assert_eq!(kernel.state().unwrap().velocity_d4, v(-ONE, 0, ONE));
    }

    #[test]
    fn missing_per_hit_external_input_is_atomic_after_lazy_initialization() {
        let input_frame = frame();
        let mut kernel = RecoveredStaticCollisionKernel::new(context(), []);
        let error = kernel
            .resolve(&input_frame, &record(5, object(0)), running())
            .unwrap_err();
        assert_eq!(
            error,
            StaticKernelError::MissingExternalInput {
                collision_ordinal: 7,
                normal_hit_index: 0,
            }
        );
        assert!(kernel.hits().is_empty());
        let state = kernel
            .state()
            .expect("narrow phase lazily initializes state");
        assert_eq!(state.position, input_frame.center);
        assert_eq!(state.hull_points, input_frame.world_hull_points);
        assert_eq!(state.projected_corners, context().projected_corners);
        assert_eq!(state.velocity_d4, context().velocity_d4);
        assert_eq!(state.out_of_control_680, context().out_of_control_680);
        assert_eq!(state.out_of_control_688, context().out_of_control_688);
    }

    #[test]
    fn normal_hit_uses_running_accumulators_and_preserves_response_event_order() {
        let external = StaticKernelExternalInputs {
            damage_result: DamageResult { effect_handle: 4 },
            ..Default::default()
        };
        let mut kernel = RecoveredStaticCollisionKernel::new(context(), [external]);
        let StaticCollisionResolution::Normal(contribution) = kernel
            .resolve(&frame(), &record(5, object(0)), running())
            .unwrap()
        else {
            panic!("normal material must resolve normally");
        };
        let StaticKernelHit::Normal {
            narrow, response, ..
        } = &kernel.hits()[0]
        else {
            panic!("normal material must take the normal path");
        };
        assert!(narrow.hit);
        assert_eq!(contribution.contact_normal, narrow.contact_normal.unwrap());
        assert_eq!(
            contribution.linear_impulse_3cc,
            response.state.linear_correction_3cc
        );
        assert_eq!(
            response.events,
            [
                StaticResponseEvent::RequestDamage {
                    linear_correction: response.state.linear_correction_3cc,
                },
                StaticResponseEvent::ContactEffect {
                    handle: 4,
                    world_contact: response.world_contact,
                },
                StaticResponseEvent::MaterialImpact {
                    severity: response.damage_severity.unwrap(),
                    world_contact: response.world_contact,
                    material: 5,
                },
                StaticResponseEvent::ScalarFeedback {
                    car_type: 0,
                    strength: response.damage_severity.unwrap().mul(q(0x1_8000)),
                    threshold: q(0x4ccc),
                },
                StaticResponseEvent::DirectionalFeedback {
                    car_type: 0,
                    direction: response.full_world_correction,
                    strength: response.damage_severity.unwrap().mul(q(2 * ONE)),
                    limit: q(2 * ONE),
                    trailing_zero: 0,
                },
            ]
        );
        assert_eq!(kernel.state().unwrap().position, narrow.final_position);
        assert_eq!(contribution.history.current_ids[0], 0);
    }

    #[test]
    fn normal_hit_uses_post_narrow_producer_normal_and_limits() {
        let mut producer_context = context();
        producer_context.contact_count_674 = 4;
        let mut input_frame = frame();
        input_frame.angular_limit_2c.x = q(i32::MIN.wrapping_add(0x1_8000));
        input_frame.angular_limit_2c.z = q(i32::MIN.wrapping_add(0x9999));
        let mut kernel = RecoveredStaticCollisionKernel::new(
            producer_context,
            [StaticKernelExternalInputs::default()],
        );
        let StaticCollisionResolution::Normal(contribution) = kernel
            .resolve(&input_frame, &record(5, object(0)), running())
            .unwrap()
        else {
            panic!("expected normal hit");
        };
        let StaticKernelHit::Normal {
            narrow,
            produced_input,
            response,
            ..
        } = &kernel.hits()[0]
        else {
            unreachable!()
        };
        let canonical = static_response_canonical_axes().axis_552f28;
        assert_ne!(narrow.contact_normal, Some(canonical));
        assert_eq!(produced_input.normal, canonical);
        assert_eq!(produced_input.projected_limit, q(0x8000));
        assert_eq!(contribution.contact_normal, produced_input.normal);
        assert_eq!(response.body_correction, response.full_world_correction);
    }

    #[test]
    fn producer_error_is_atomic_and_does_not_consume_external_input() {
        let mut producer_context = context();
        producer_context.contact_count_674 = 4;
        producer_context.velocity_normalizer_ec = q(1);
        let mut input_frame = frame();
        input_frame.angular_limit_2c.x = q(i32::MIN.wrapping_add(0x1_8000));
        input_frame.angular_limit_2c.z = q(i32::MIN.wrapping_add(0x9999));
        let mut kernel = RecoveredStaticCollisionKernel::new(
            producer_context,
            [StaticKernelExternalInputs::default()],
        );
        let error = kernel
            .resolve(&input_frame, &record(5, object(0)), running())
            .unwrap_err();
        assert_eq!(
            error,
            StaticKernelError::ResponseInputProducer {
                collision_ordinal: 7,
                source: StaticResponseInputProducerError::ReciprocalOverflow { divisor: 1 },
            }
        );
        assert!(kernel.hits.is_empty());
        assert_eq!(kernel.external_inputs.len(), 1);
        let state = kernel.state.unwrap();
        assert_eq!(state.position, input_frame.center);
        assert_eq!(state.hull_points, input_frame.world_hull_points);
    }

    #[test]
    fn below_threshold_effect_handle_is_rejected_atomically() {
        let mut zero_context = context();
        zero_context.velocity_d4 = Vec3::ZERO;
        let external = StaticKernelExternalInputs {
            damage_result: DamageResult { effect_handle: 9 },
            ..Default::default()
        };
        let mut zero_running = running();
        zero_running.linear_impulse_3cc = Vec3::ZERO;
        let input_frame = frame();
        let mut kernel = RecoveredStaticCollisionKernel::new(zero_context, [external]);
        let error = kernel
            .resolve(&input_frame, &record(5, object(0)), zero_running)
            .unwrap_err();
        assert_eq!(
            error,
            StaticKernelError::ImpossibleEffectHandle {
                collision_ordinal: 7,
                effect_handle: 9,
            }
        );
        assert!(kernel.hits.is_empty());
        assert_eq!(kernel.external_inputs.len(), 1);
        let state = kernel.state.unwrap();
        assert_eq!(state.position, input_frame.center);
        assert_eq!(state.velocity_d4, Vec3::ZERO);
    }

    #[test]
    fn first_entered_damage_branch_effect_handle_commits_atomically() {
        let threshold_input = (0..0x4000)
            .find(|raw| evaluate_static_damage(v(*raw, 0, 0)).damage_sequence_invoked)
            .expect("threshold must be reachable on one axis");
        let damage = evaluate_static_damage(v(threshold_input, 0, 0));
        assert!(damage.damage_sequence_invoked);
        assert!(damage.severity > Fixed16::ZERO);

        let mut zero_context = context();
        zero_context.velocity_d4 = Vec3::ZERO;
        let external = StaticKernelExternalInputs {
            damage_result: DamageResult { effect_handle: 9 },
            ..Default::default()
        };
        let mut entered_running = running();
        entered_running.linear_impulse_3cc = v(threshold_input, 0, 0);
        let input_frame = frame();
        let mut kernel = RecoveredStaticCollisionKernel::new(zero_context, [external]);
        let StaticCollisionResolution::Normal(_) = kernel
            .resolve(&input_frame, &record(5, object(0)), entered_running)
            .unwrap()
        else {
            panic!("the first entered damage branch must commit");
        };
        assert!(kernel.external_inputs.is_empty());
        let StaticKernelHit::Normal {
            narrow, response, ..
        } = &kernel.hits[0]
        else {
            unreachable!()
        };
        assert_eq!(kernel.state().unwrap().position, narrow.final_position);
        assert!(response.damage_sequence_invoked);
        assert_eq!(response.damage_severity, Some(damage.severity));
        assert!(
            response
                .events
                .contains(&StaticResponseEvent::ContactEffect {
                    handle: 9,
                    world_contact: response.world_contact,
                })
        );
    }

    #[test]
    fn alternate_hit_updates_only_alternate_outputs_and_returns_unchanged_running_state() {
        let mut kernel = RecoveredStaticCollisionKernel::new(context(), []);
        let initial = running();
        let resolution = kernel
            .resolve(&frame(), &record(0x85, object(0)), initial)
            .unwrap();
        assert_eq!(resolution, StaticCollisionResolution::Alternate);
        let StaticKernelHit::Alternate { narrow, .. } = kernel.hits()[0] else {
            panic!("bit 0x80 must take the alternate path");
        };
        assert_eq!(kernel.state().unwrap().velocity_d4, narrow.velocity_d4);
        assert_eq!(kernel.state().unwrap().alternate_point_flags_69c, [1; 8]);
    }

    #[test]
    fn material_low_bits_do_not_select_the_path_but_reach_normal_response() {
        let mut normal =
            RecoveredStaticCollisionKernel::new(context(), [StaticKernelExternalInputs::default()]);
        normal
            .resolve(&frame(), &record(0x7f, object(0)), running())
            .unwrap();
        let StaticKernelHit::Normal { response, .. } = &normal.hits()[0] else {
            panic!("clear bit 0x80 must take the normal path");
        };
        assert_eq!(response.debug_material_write, None);

        let mut alternate = RecoveredStaticCollisionKernel::new(context(), []);
        alternate
            .resolve(&frame(), &record(0xff, object(0)), running())
            .unwrap();
        assert!(matches!(
            alternate.hits()[0],
            StaticKernelHit::Alternate { .. }
        ));
    }

    #[test]
    fn scene_alternate_then_zero_normal_retains_both_but_only_normal_updates_history() {
        use crate::static_world_collision::{
            StaticCollisionNodeId, StaticCollisionScene, StaticResolvedContact,
            resolve_static_world_collisions,
        };

        let node = StaticCollisionNodeId::new(3).unwrap();
        let scene = StaticCollisionScene::new(
            vec![
                record_with_id(10, 0x85, object(0)),
                record_with_id(11, 5, object(0)),
            ],
            [(node, 1), (node, 0)],
        )
        .unwrap();
        let mut input_frame = frame();
        input_frame.previous_center_xz = [Fixed16::ZERO; 2];
        input_frame.active_nodes = [Some(node), None, None, None];
        let mut kernel =
            RecoveredStaticCollisionKernel::new(context(), [StaticKernelExternalInputs::default()]);
        let effects =
            resolve_static_world_collisions(&scene, &input_frame, Default::default(), &mut kernel)
                .unwrap();

        assert_eq!(
            effects.contacts,
            [
                StaticResolvedContact::Alternate {
                    collision_ordinal: 10,
                    raw_material_id: 5,
                },
                StaticResolvedContact::Normal {
                    collision_ordinal: 11,
                    contact_history_id: 0,
                    raw_material_id: 5,
                    contact_normal: Vec3::ZERO,
                    repeated_from_previous_tick: false,
                    stored_in_current_history: true,
                },
            ]
        );
        assert_eq!(effects.history.current_len, 1);
        assert_eq!(effects.history.current_ids[0], 0);
        let StaticKernelHit::Normal { response, .. } = &kernel.hits()[1] else {
            panic!("second hit must take the normal response path");
        };
        assert_eq!(response.full_world_correction, Vec3::ZERO);
    }

    #[test]
    fn two_normals_accumulate_sequentially_and_consume_distinct_external_results() {
        use crate::static_collision_response::StaticResponseEvent;
        use crate::static_world_collision::{
            StaticCollisionNodeId, StaticCollisionScene, resolve_static_world_collisions,
        };

        let node = StaticCollisionNodeId::new(4).unwrap();
        let scene = StaticCollisionScene::new(
            vec![
                record_with_id(20, 5, object(0)),
                record_with_id(21, 6, object(-2 * ONE)),
            ],
            [(node, 1), (node, 0)],
        )
        .unwrap();
        let mut input_frame = frame();
        input_frame.active_nodes = [Some(node), None, None, None];
        input_frame.linear_impulse_3cc = Vec3::ZERO;
        let external = [11, 22].map(|handle| StaticKernelExternalInputs {
            damage_result: DamageResult {
                effect_handle: handle,
            },
            ..Default::default()
        });
        let mut kernel = RecoveredStaticCollisionKernel::new(context(), external);
        let effects =
            resolve_static_world_collisions(&scene, &input_frame, Default::default(), &mut kernel)
                .unwrap();

        assert_eq!(effects.contacts.len(), 2);
        assert_eq!(effects.history.current_len, 2);
        let handles: Vec<i32> = kernel
            .hits()
            .iter()
            .map(|hit| match hit {
                StaticKernelHit::Normal { response, .. } => response
                    .events
                    .iter()
                    .find_map(|event| match event {
                        StaticResponseEvent::ContactEffect { handle, .. } => Some(*handle),
                        _ => None,
                    })
                    .unwrap(),
                StaticKernelHit::Alternate { .. } => panic!("expected normal hit"),
            })
            .collect();
        assert_eq!(handles, [11, 22]);
        let StaticKernelHit::Normal {
            response: first_response,
            ..
        } = &kernel.hits()[0]
        else {
            unreachable!()
        };
        let StaticKernelHit::Normal {
            response: second_response,
            ..
        } = &kernel.hits()[1]
        else {
            unreachable!()
        };
        let expected = first_response
            .full_world_correction
            .add(second_response.full_world_correction);
        assert_eq!(effects.linear_impulse_3cc, expected);
        assert_eq!(second_response.state.linear_correction_3cc, expected);
        assert_eq!(
            first_response.damage_severity,
            Some(static_damage_severity(
                first_response.state.linear_correction_3cc
            ))
        );
        assert_eq!(
            second_response.damage_severity,
            Some(static_damage_severity(
                second_response.state.linear_correction_3cc
            ))
        );
        assert_ne!(
            second_response.damage_severity,
            Some(static_damage_severity(
                second_response.full_world_correction
            )),
            "second severity must observe the total accumulated +3cc"
        );
    }

    #[test]
    fn next_tick_rotates_history_and_suppresses_repeated_damage_request() {
        use crate::static_collision_response::StaticResponseEvent;
        use crate::static_world_collision::{
            StaticCollisionNodeId, StaticCollisionScene, resolve_static_world_collisions,
        };

        let node = StaticCollisionNodeId::new(5).unwrap();
        let scene =
            StaticCollisionScene::new(vec![record_with_id(30, 5, object(0))], [(node, 0)]).unwrap();
        let mut input_frame = frame();
        input_frame.active_nodes = [Some(node), None, None, None];
        let external = StaticKernelExternalInputs {
            damage_result: DamageResult { effect_handle: 9 },
            ..Default::default()
        };
        let mut type_one_context = context();
        type_one_context.car_type_738 = 1;
        let mut first_kernel = RecoveredStaticCollisionKernel::new(type_one_context, [external]);
        let first = resolve_static_world_collisions(
            &scene,
            &input_frame,
            Default::default(),
            &mut first_kernel,
        )
        .unwrap();
        assert!(matches!(
            first_kernel.hits()[0],
            StaticKernelHit::Normal { ref response, .. }
                if response.events.iter().any(|event|
                    matches!(event, StaticResponseEvent::RequestDamage { .. }))
        ));

        let repeated_external = StaticKernelExternalInputs {
            type_one_effect_gate_blocked: true,
            ..external
        };
        let mut second_kernel =
            RecoveredStaticCollisionKernel::new(type_one_context, [repeated_external]);
        let second = resolve_static_world_collisions(
            &scene,
            &input_frame,
            first.history,
            &mut second_kernel,
        )
        .unwrap();
        let StaticKernelHit::Normal { response, .. } = &second_kernel.hits()[0] else {
            unreachable!()
        };
        assert!(response.previous_contact);
        assert_eq!(response.damage_severity, None);
        assert_eq!(second_kernel.external_inputs.len(), 0);
        assert!(matches!(
            response.events.first(),
            Some(StaticResponseEvent::QueryTypeOneEffectGate)
        ));
        assert!(
            response
                .events
                .iter()
                .all(|event| !matches!(event, StaticResponseEvent::RequestDamage { .. }))
        );
        assert_eq!(second.history.previous_len, 1);
        assert_eq!(second.history.previous_ids[0], 0);
        assert_eq!(second.history.current_len, 1);
    }

    #[test]
    fn live_owner_is_not_called_below_threshold_or_for_repeated_history() {
        let below_owner = RecordingDamageOwner::returning([1]);
        let below_calls = Rc::clone(&below_owner.calls);
        let mut below_context = context();
        below_context.velocity_d4 = Vec3::ZERO;
        let mut below_running = running();
        below_running.linear_impulse_3cc = Vec3::ZERO;
        let mut below =
            RecoveredStaticCollisionKernel::with_damage_owner(below_context, [], below_owner);
        let resolution = below
            .resolve(&frame(), &record(5, object(0)), below_running)
            .unwrap();
        assert!(matches!(resolution, StaticCollisionResolution::Normal(_)));
        assert!(below_calls.borrow().is_empty());

        let repeated_owner = RecordingDamageOwner::returning([2]);
        let repeated_calls = Rc::clone(&repeated_owner.calls);
        let mut repeated_running = running();
        repeated_running.history.push(STATIC_RESPONSE_CONTACT_ID);
        repeated_running.history = repeated_running.history.begin_tick();
        let mut repeated =
            RecoveredStaticCollisionKernel::with_damage_owner(context(), [], repeated_owner);
        let resolution = repeated
            .resolve(&frame(), &record(5, object(0)), repeated_running)
            .unwrap();
        assert!(matches!(resolution, StaticCollisionResolution::Normal(_)));
        assert!(repeated_calls.borrow().is_empty());
    }

    #[test]
    fn threshold_equality_calls_live_owner_with_zero_severity() {
        let owner = RecordingDamageOwner::returning([3]);
        let calls = Rc::clone(&owner.calls);
        let mut kernel = RecoveredStaticCollisionKernel::with_damage_owner(context(), [], owner);
        let equality = evaluate_static_damage_length(0x2624);
        assert!(equality.damage_sequence_invoked);
        assert_eq!(equality.severity, Fixed16::ZERO);

        let result = kernel
            .obtain_damage_result(17, Some(equality), v(1, 2, 3), v(4, 5, 6), None)
            .unwrap();
        assert_eq!(result, DamageResult { effect_handle: 3 });
        assert_eq!(
            calls.borrow().as_slice(),
            [StaticDamageRequest {
                car_type_738: 0,
                car_subtype_739: 0,
                contact_3f0: v(1, 2, 3),
                linear_correction_3cc: v(4, 5, 6),
                severity: Fixed16::ZERO,
            }]
        );
    }

    #[test]
    fn live_owner_handles_two_normal_hits_lazily_with_empty_replay_fifo() {
        use crate::static_world_collision::{
            StaticCollisionNodeId, StaticCollisionScene, resolve_static_world_collisions,
        };

        let node = StaticCollisionNodeId::new(6).unwrap();
        let scene = StaticCollisionScene::new(
            vec![
                record_with_id(40, 5, object(0)),
                record_with_id(41, 6, object(-2 * ONE)),
            ],
            [(node, 1), (node, 0)],
        )
        .unwrap();
        let mut input_frame = frame();
        input_frame.active_nodes = [Some(node), None, None, None];
        input_frame.linear_impulse_3cc = Vec3::ZERO;
        let owner = RecordingDamageOwner::returning([1, 2]);
        let calls = Rc::clone(&owner.calls);
        let mut kernel = RecoveredStaticCollisionKernel::with_damage_owner(context(), [], owner);

        let effects =
            resolve_static_world_collisions(&scene, &input_frame, Default::default(), &mut kernel)
                .unwrap();
        assert_eq!(effects.contacts.len(), 2);
        assert_eq!(calls.borrow().len(), 2);
        assert!(kernel.external_inputs.is_empty());
        assert_ne!(
            calls.borrow()[0].linear_correction_3cc,
            calls.borrow()[1].linear_correction_3cc,
            "the second lazy request must observe sequential accumulation"
        );
    }

    #[test]
    fn live_owner_error_leaves_kernel_contact_state_atomic() {
        let input_frame = frame();
        let owner = RecordingDamageOwner::failing();
        let calls = Rc::clone(&owner.calls);
        let mut kernel = RecoveredStaticCollisionKernel::with_damage_owner(context(), [], owner);

        let error = kernel
            .resolve(&input_frame, &record(5, object(0)), running())
            .unwrap_err();
        assert_eq!(
            error,
            StaticKernelError::DamageOwner {
                collision_ordinal: 7,
                source: TestDamageError,
            }
        );
        assert_eq!(calls.borrow().len(), 1);
        assert!(kernel.hits().is_empty());
        let state = kernel.state().expect("narrow phase initializes the cache");
        assert_eq!(state.position, input_frame.center);
        assert_eq!(state.hull_points, input_frame.world_hull_points);
        assert_eq!(state.projected_corners, context().projected_corners);
        assert_eq!(state.velocity_d4, context().velocity_d4);
        assert!(kernel.external_inputs.is_empty());
    }
}
