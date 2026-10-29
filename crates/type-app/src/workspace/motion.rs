use std::time::{Duration, Instant};

pub(super) struct Motion {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
}

impl Motion {
    pub(super) fn new(from: f32, to: f32, started: Instant, duration: Duration) -> Self {
        Self {
            from,
            to,
            started,
            duration,
        }
    }

    pub(super) fn value(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.started).as_secs_f32()
            / self.duration.as_secs_f32())
        .min(1.);
        self.from + (self.to - self.from) * (1. - (1. - t).powi(3))
    }

    pub(super) fn moving(&self, now: Instant) -> bool {
        self.from != self.to && now.saturating_duration_since(self.started) < self.duration
    }
    pub(super) fn retarget(&mut self, target: f32, now: Instant, duration: Duration) {
        if target != self.to {
            *self = Self::new(self.value(now), target, now, duration);
        }
    }
}
