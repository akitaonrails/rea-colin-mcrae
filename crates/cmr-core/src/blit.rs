#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectI16 {
    pub x: i16,
    pub y: i16,
    pub width: i16,
    pub height: i16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectEdges {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColorKey {
    pub low: u32,
    pub high: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlitMethod {
    Fast,
    Stretch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlitPlan {
    pub source: RectEdges,
    pub destination: RectEdges,
    pub method: BlitMethod,
    pub color_key: Option<ColorKey>,
}

impl BlitPlan {
    /// DirectDraw flags observed at the recovered call boundary.
    pub fn legacy_flags(self) -> u32 {
        match (self.method, self.color_key.is_some()) {
            (BlitMethod::Fast, false) => 0x10,
            (BlitMethod::Fast, true) => 0x11,
            (BlitMethod::Stretch, false) => 0x0100_0000,
            (BlitMethod::Stretch, true) => 0x0100_8000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlitDecision {
    /// The legacy routine treats a fully clipped destination as success without
    /// making a graphics API call.
    NoOp,
    Blit(BlitPlan),
}

/// Reproduces the recovered destination clipping and dispatch decision.
///
/// The legacy implementation shifts source edges by literal destination pixel
/// deltas even for stretched blits, does not clip against source bounds, and
/// decides Fast versus Stretch from the original (pre-clipping) dimensions.
pub fn plan_legacy_blit(
    source: RectI16,
    destination: RectI16,
    destination_width: i16,
    destination_height: i16,
    color_key: Option<ColorKey>,
) -> BlitDecision {
    let mut source_edges = edges(source);
    let mut destination_edges = edges(destination);
    let width = i32::from(destination_width);
    let height = i32::from(destination_height);
    let method = if source.width == destination.width && source.height == destination.height {
        BlitMethod::Fast
    } else {
        BlitMethod::Stretch
    };

    if destination_edges.left >= width
        || destination_edges.right <= 0
        || destination_edges.top >= height
        || destination_edges.bottom <= 0
    {
        return BlitDecision::NoOp;
    }

    if destination_edges.left < 0 {
        source_edges.left -= destination_edges.left;
        destination_edges.left = 0;
    }
    if destination_edges.right > width {
        source_edges.right += width - destination_edges.right;
        destination_edges.right = width;
    }
    if destination_edges.top < 0 {
        source_edges.top -= destination_edges.top;
        destination_edges.top = 0;
    }
    if destination_edges.bottom > height {
        source_edges.bottom += height - destination_edges.bottom;
        destination_edges.bottom = height;
    }

    BlitDecision::Blit(BlitPlan {
        source: source_edges,
        destination: destination_edges,
        method,
        color_key,
    })
}

fn edges(rectangle: RectI16) -> RectEdges {
    let left = i32::from(rectangle.x);
    let top = i32::from(rectangle.y);
    RectEdges {
        left,
        top,
        right: left + i32::from(rectangle.width),
        bottom: top + i32::from(rectangle.height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i16, y: i16, width: i16, height: i16) -> RectI16 {
        RectI16 {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn same_size_inside_destination_selects_fast_path() {
        assert_eq!(
            plan_legacy_blit(rect(1, 2, 10, 20), rect(3, 4, 10, 20), 100, 100, None),
            BlitDecision::Blit(BlitPlan {
                source: RectEdges {
                    left: 1,
                    top: 2,
                    right: 11,
                    bottom: 22,
                },
                destination: RectEdges {
                    left: 3,
                    top: 4,
                    right: 13,
                    bottom: 24,
                },
                method: BlitMethod::Fast,
                color_key: None,
            })
        );
    }

    #[test]
    fn clips_every_destination_edge_and_shifts_source_one_for_one() {
        assert_eq!(
            plan_legacy_blit(rect(10, 20, 40, 50), rect(-3, -4, 40, 50), 30, 35, None),
            BlitDecision::Blit(BlitPlan {
                source: RectEdges {
                    left: 13,
                    top: 24,
                    right: 43,
                    bottom: 59,
                },
                destination: RectEdges {
                    left: 0,
                    top: 0,
                    right: 30,
                    bottom: 35,
                },
                method: BlitMethod::Fast,
                color_key: None,
            })
        );
    }

    #[test]
    fn stretch_clipping_is_not_proportional_and_method_is_not_recomputed() {
        let decision = plan_legacy_blit(rect(0, 0, 100, 100), rect(-10, 0, 50, 100), 40, 100, None);
        let BlitDecision::Blit(plan) = decision else {
            panic!("expected a blit");
        };
        assert_eq!(plan.method, BlitMethod::Stretch);
        assert_eq!(plan.source.left, 10);
        assert_eq!(plan.source.right, 100);
        assert_eq!(plan.destination.left, 0);
        assert_eq!(plan.destination.right, 40);
        assert_eq!(plan.legacy_flags(), 0x0100_0000);
    }

    #[test]
    fn stretch_clipping_shifts_vertical_edges_by_literal_pixels() {
        let decision = plan_legacy_blit(rect(5, 10, 50, 100), rect(0, -7, 25, 60), 25, 40, None);
        let BlitDecision::Blit(plan) = decision else {
            panic!("expected a blit");
        };
        assert_eq!(plan.method, BlitMethod::Stretch);
        assert_eq!(plan.source.top, 17);
        assert_eq!(plan.source.bottom, 97);
        assert_eq!(plan.destination.top, 0);
        assert_eq!(plan.destination.bottom, 40);
    }

    #[test]
    fn touching_or_fully_outside_destinations_are_successful_no_ops() {
        for destination in [
            rect(-10, 0, 10, 10),
            rect(100, 0, 10, 10),
            rect(0, -10, 10, 10),
            rect(0, 100, 10, 10),
        ] {
            assert_eq!(
                plan_legacy_blit(rect(0, 0, 10, 10), destination, 100, 100, None),
                BlitDecision::NoOp
            );
        }
    }

    #[test]
    fn color_key_selects_the_observed_directdraw_flags() {
        let key = Some(ColorKey { low: 2, high: 7 });
        let BlitDecision::Blit(fast) =
            plan_legacy_blit(rect(0, 0, 10, 10), rect(0, 0, 10, 10), 100, 100, key)
        else {
            panic!("expected fast blit");
        };
        let BlitDecision::Blit(stretch) =
            plan_legacy_blit(rect(0, 0, 9, 10), rect(0, 0, 10, 10), 100, 100, key)
        else {
            panic!("expected stretch blit");
        };
        assert_eq!(fast.legacy_flags(), 0x11);
        assert_eq!(stretch.legacy_flags(), 0x0100_8000);
    }
}
