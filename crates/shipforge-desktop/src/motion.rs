//! Native equivalent of prototypes/workbench/motion.js; one owner for spatial changes.
use gpui_kit::{prelude::*, *};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub fn spring(t: f32) -> f32 {
    if t >= 1. {
        1.
    } else {
        1. - (-10. * t).exp() * ((12. * t).cos() + (10. / 12.) * (12. * t).sin())
    }
}
#[derive(Clone, Copy)]
struct Value {
    from: f32,
    target: f32,
    started: Instant,
    millis: f32,
}
impl Value {
    fn new(target: f32, now: Instant) -> Self {
        Self {
            from: target,
            target,
            started: now,
            millis: SURFACE_DURATION,
        }
    }
    fn sample(self, now: Instant, linear: bool) -> f32 {
        let t = (now.duration_since(self.started).as_secs_f32() * 1000. / self.millis).min(1.);
        self.from + (self.target - self.from) * if linear { t } else { spring(t) }
    }
    fn active(self, now: Instant) -> bool {
        self.from != self.target
            && now.duration_since(self.started).as_millis() < self.millis as u128
    }
    fn retarget(&mut self, target: f32, now: Instant, linear: bool) {
        if self.target != target {
            self.from = self.sample(now, linear);
            self.target = target;
            self.started = now;
        }
    }
}
#[derive(Default)]
pub struct Motion {
    channels: HashMap<String, Value>,
}
pub struct Timing {
    millis: f32,
    initial: Option<f32>,
    linear: bool,
}
impl Timing {
    pub const FOCUS: Self = Self {
        millis: 200.,
        initial: None,
        linear: true,
    };
    fn spring(millis: f32, initial: Option<f32>) -> Self {
        Self {
            millis,
            initial,
            linear: false,
        }
    }
}
impl Motion {
    pub fn value(
        &mut self,
        key: impl Into<String>,
        target: f32,
        timing: Timing,
        window: &mut Window,
        cx: &App,
    ) -> f32 {
        let Timing {
            millis,
            initial,
            linear,
        } = timing;
        let now = Instant::now();
        let value = self
            .channels
            .entry(key.into())
            .or_insert_with(|| Value::new(initial.unwrap_or(target), now));
        value.retarget(target, now, linear);
        value.millis = millis;
        if cx.reduce_motion() && !linear {
            value.from = target;
            return target;
        }
        if value.active(now) {
            window.request_animation_frame();
        }
        value.sample(now, linear)
    }
    pub fn size(
        &mut self,
        key: impl Into<String>,
        target: f32,
        window: &mut Window,
        cx: &App,
    ) -> f32 {
        self.value(
            key,
            target,
            Timing::spring(SURFACE_DURATION, None),
            window,
            cx,
        )
    }
    pub fn presence(
        &mut self,
        key: impl Into<String>,
        present: bool,
        millis: f32,
        window: &mut Window,
        cx: &App,
    ) -> f32 {
        self.value(
            key,
            if present { 1. } else { 0. },
            Timing::spring(millis, Some(0.)),
            window,
            cx,
        )
    }
    pub fn settle(&mut self) {
        for v in self.channels.values_mut() {
            v.from = v.target;
        }
    }
}

/// Reorder/entry without scaling text. Scroll offsets remain immediate; only a layout
/// revision starts a new spring. Hitboxes are painted at the same translated position.
pub fn layout(key: impl Into<ElementId>, revision: u64, child: impl IntoElement) -> Layout {
    Layout {
        id: key.into(),
        revision,
        child: child.into_any_element(),
    }
}
pub struct Layout {
    id: ElementId,
    revision: u64,
    child: AnyElement,
}
struct Geometry {
    origin: Point<Pixels>,
    viewport: Size<Pixels>,
    revision: u64,
    x: Value,
    y: Value,
}
impl IntoElement for Layout {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Layout {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let now = Instant::now();
        let viewport = window.viewport_size();
        let (offset, active) = window.with_element_state(
            id.expect("stable motion key"),
            |state: Option<Geometry>, _| {
                let mut state = state.unwrap_or_else(|| Geometry {
                    origin: bounds.origin,
                    viewport,
                    revision: self.revision,
                    x: Value::new(0., now),
                    y: Value {
                        from: 8.,
                        target: 0.,
                        started: now,
                        millis: SURFACE_DURATION,
                    },
                });
                if state.revision != self.revision {
                    state.x.from =
                        state.x.sample(now, false) + f32::from(state.origin.x - bounds.origin.x);
                    state.y.from =
                        state.y.sample(now, false) + f32::from(state.origin.y - bounds.origin.y);
                    state.x.started = now;
                    state.y.started = now;
                    state.revision = self.revision;
                }
                state.origin = bounds.origin;
                if cx.reduce_motion() || state.viewport != viewport {
                    state.x.from = 0.;
                    state.y.from = 0.;
                }
                state.viewport = viewport;
                let active = state.x.active(now) || state.y.active(now);
                (
                    (
                        point(
                            px(state.x.sample(now, false)),
                            px(state.y.sample(now, false)),
                        ),
                        active,
                    ),
                    state,
                )
            },
        );
        if active {
            window.request_animation_frame();
        }
        window.with_element_offset(offset, |window| {
            self.child.prepaint(window, cx);
        });
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

pub const SURFACE_DURATION: f32 = 540.;
pub const ENTER_DURATION: f32 = 440.;
pub const EXIT_DURATION: f32 = 240.;
pub const SYSTEM_POLL: Duration = Duration::from_millis(250);

#[cfg(test)]
mod tests {
    use super::{Value, spring};
    use std::time::{Duration, Instant};
    #[test]
    fn reversal_is_continuous_and_has_exact_endpoint() {
        let now = Instant::now();
        let mut value = Value::new(0., now);
        value.retarget(200., now, false);
        let midway = now + Duration::from_millis(90);
        let before = value.sample(midway, false);
        value.retarget(0., midway, false);
        assert!((before - value.sample(midway, false)).abs() < 0.0001);
        assert_eq!(value.sample(midway + Duration::from_millis(540), false), 0.);
        assert_eq!(spring(0.), 0.);
        assert_eq!(spring(1.), 1.);
        assert!(spring(0.28) > 1.);
    }
}
