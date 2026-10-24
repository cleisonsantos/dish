//! Short, event-driven transitions on GPUI's GPU-rendered scene.
//!
//! GPUI interpolates on its frame clock, not on a timer thread. One-shot
//! animations stop requesting frames at rest. Its AnimationExt also honours
//! App::reduce_motion, including the repeating live-work indicators.

use gpui::{Animation, AnimationExt, ElementId, IntoElement, Styled, px};
use std::time::Duration;

pub const ENTER_MS: u64 = 180;
pub const PANEL_MS: u64 = 220;

/// A monotonic stop: quick acknowledgement, no bounce or overshoot.
pub fn settle(progress: f32) -> f32 {
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(3)
}

pub fn reveal<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
) -> impl IntoElement {
    element.with_animation(
        id,
        Animation::new(Duration::from_millis(ENTER_MS)).with_easing(settle),
        |element, progress| element.opacity(progress),
    )
}

/// New content rises into place: a few pixels and a fade, layout untouched.
pub fn rise<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
) -> impl IntoElement {
    element.with_animation(
        id,
        Animation::new(Duration::from_millis(PANEL_MS)).with_easing(settle),
        |element, progress| {
            element
                .relative()
                .top(px(10.0 * (1.0 - progress)))
                .opacity(progress)
        },
    )
}

/// Offsets only the panel's position; its final layout space stays reserved.
pub fn panel<E: IntoElement + Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
    from_right: bool,
) -> impl IntoElement {
    element.with_animation(
        id,
        Animation::new(Duration::from_millis(PANEL_MS)).with_easing(settle),
        move |element, progress| {
            let direction = if from_right { 1.0 } else { -1.0 };
            element
                .relative()
                .left(px(direction * 12.0 * (1.0 - progress)))
                .opacity(progress)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_is_bounded_monotonic_and_finishes_exactly() {
        assert_eq!(settle(-1.), 0.);
        assert_eq!(settle(0.), 0.);
        assert_eq!(settle(1.), 1.);
        assert_eq!(settle(2.), 1.);
        let mut previous = 0.;
        for step in 0..=100 {
            let value = settle(step as f32 / 100.);
            assert!((previous..=1.).contains(&value));
            previous = value;
        }
    }

    #[test]
    fn motion_responds_without_a_long_entrance() {
        assert!(settle(0.5) > 0.8);
        let durations = [ENTER_MS, PANEL_MS];
        assert!(durations.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(durations.into_iter().all(|duration| duration <= 250));
    }
}
