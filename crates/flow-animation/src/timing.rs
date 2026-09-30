//! Pure, bounded time mapping. Duration arithmetic stays integer-valued until
//! sampling one keyframe's existing easing function.
use crate::{AnimationError, AnimationSequence, AnimationSpec};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};

/// A positive rational playback rate. For example, 3/2 is 1.5× speed.
/// The nonzero components also reject zero denominators during deserialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackRate {
    numerator: NonZeroU32,
    denominator: NonZeroU32,
}
impl PlaybackRate {
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, AnimationError> {
        let numerator = NonZeroU32::new(numerator)
            .ok_or_else(|| invalid("playback rate numerator must be positive"))?;
        let denominator = NonZeroU32::new(denominator)
            .ok_or_else(|| invalid("playback rate denominator must be positive"))?;
        Ok(Self {
            numerator,
            denominator,
        })
    }
    pub fn numerator(self) -> u32 {
        self.numerator.get()
    }
    pub fn denominator(self) -> u32 {
        self.denominator.get()
    }
}
impl Default for PlaybackRate {
    fn default() -> Self {
        Self {
            numerator: NonZeroU32::MIN,
            denominator: NonZeroU32::MIN,
        }
    }
}

/// Total traversals, not extra repeats: Finite(1) plays once; Finite(2) twice.
/// Finite(0) is invalid. Infinite playback ends only through explicit lifecycle
/// operations; reaching the representable clock limit returns an atomic error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "count", rename_all = "snake_case")]
pub enum IterationCount {
    Finite(u64),
    Infinite,
}
impl Default for IterationCount {
    fn default() -> Self {
        Self::Finite(1)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackDirection {
    #[default]
    Forward,
    Reverse,
    /// Forward, then backward, repeating. Two iterations make one round trip.
    Alternate,
    AlternateReverse,
}
impl PlaybackDirection {
    fn reversed(self, iteration: u128) -> bool {
        match self {
            Self::Forward => false,
            Self::Reverse => true,
            Self::Alternate => iteration % 2 == 1,
            Self::AlternateReverse => iteration.is_multiple_of(2),
        }
    }
}

/// Timing around a positive-duration sequence. Delays and seek positions use
/// logical time; tick deltas are scaled by speed. No wall-clock timers are used.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnimationTiming {
    pub delay_ms: u64,
    /// Hold each traversal's endpoint before the next traversal. There is no
    /// repeat delay after the last finite iteration.
    pub repeat_delay_ms: u64,
    pub iterations: IterationCount,
    pub direction: PlaybackDirection,
    pub speed: PlaybackRate,
}
impl AnimationTiming {
    /// Finite logical length including delays, or None for infinite repetition.
    /// Rejects zero-duration cycles and totals outside Duration's finite range.
    pub fn total_duration(self, cycle: Duration) -> Result<Option<Duration>, AnimationError> {
        Ok(self.compile(cycle)?.total)
    }
    /// Sample an absolute logical position, independent of playback speed. A
    /// finite position clamps to the end; infinite positions do not clamp.
    pub fn sample(
        self,
        cycle: Duration,
        position: Duration,
    ) -> Result<TimingSample, AnimationError> {
        Ok(self.compile(cycle)?.sample(position))
    }
    pub(crate) fn compile(self, cycle: Duration) -> Result<Schedule, AnimationError> {
        if cycle.is_zero() {
            return Err(invalid("positive cycle duration required"));
        }
        let delay = Duration::from_millis(self.delay_ms);
        let gap = Duration::from_millis(self.repeat_delay_ms);
        // This sum fits u128 even when one input is Duration::MAX. It need not
        // fit Duration: a finite single pass does not use the repeat gap.
        let stride_ns = cycle.as_nanos() + gap.as_nanos();
        let total = match self.iterations {
            IterationCount::Finite(0) => return Err(invalid("at least one iteration required")),
            IterationCount::Finite(count) => {
                let nanos = cycle
                    .as_nanos()
                    .checked_mul(u128::from(count))
                    .and_then(|n| {
                        gap.as_nanos()
                            .checked_mul(u128::from(count - 1))
                            .and_then(|g| n.checked_add(g))
                    })
                    .and_then(|n| n.checked_add(delay.as_nanos()))
                    .ok_or_else(|| invalid("animation total duration overflow"))?;
                Some(duration_from_nanos(
                    nanos,
                    "animation total duration overflow",
                )?)
            }
            IterationCount::Infinite => None,
        };
        Ok(Schedule {
            timing: self,
            cycle,
            delay,
            stride_ns,
            total,
        })
    }
}

/// Declarative composition of one bounded keyframe sequence and its timing.
/// Multiple independent clips share a controller tick/PlaybackToken. This is not
/// a nested timeline: no implicit offsets, blending, callbacks, or child clocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationClip {
    pub sequence: AnimationSequence,
    #[serde(default)]
    pub timing: AnimationTiming,
}
impl AnimationClip {
    pub fn new(sequence: AnimationSequence) -> Self {
        Self {
            sequence,
            timing: AnimationTiming::default(),
        }
    }
    pub fn with_timing(mut self, timing: AnimationTiming) -> Self {
        self.timing = timing;
        self
    }
}
impl From<AnimationSpec> for AnimationClip {
    fn from(spec: AnimationSpec) -> Self {
        Self::new(AnimationSequence::new(vec![spec]))
    }
}
impl From<AnimationSequence> for AnimationClip {
    fn from(sequence: AnimationSequence) -> Self {
        Self::new(sequence)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingPhase {
    /// Holds the captured original pose, including for reverse playback.
    Delay,
    Active,
    RepeatDelay,
    Completed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimingSample {
    pub phase: TimingPhase,
    /// Clamped logical timeline position, including all delays.
    pub elapsed: Duration,
    pub total_duration: Option<Duration>,
    /// Zero-based traversal index. u128 preserves huge infinite-loop indices.
    pub iteration: u128,
    /// Position in the sequence's forward coordinate system. During Delay this
    /// is zero and reversed is false, so the captured pose is held.
    pub cycle_position: Duration,
    /// Reverse playback applies each segment's easing in travel direction.
    pub reversed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Schedule {
    pub timing: AnimationTiming,
    cycle: Duration,
    delay: Duration,
    stride_ns: u128,
    pub total: Option<Duration>,
}
impl Schedule {
    pub fn sample(&self, position: Duration) -> TimingSample {
        let elapsed = self.total.map_or(position, |total| position.min(total));
        let (phase, iteration, progress) = if elapsed < self.delay {
            (TimingPhase::Delay, 0, Duration::ZERO)
        } else if self.total == Some(elapsed) {
            let IterationCount::Finite(count) = self.timing.iterations else {
                unreachable!("only finite schedules have a total")
            };
            (TimingPhase::Completed, u128::from(count - 1), self.cycle)
        } else {
            let active = elapsed.saturating_sub(self.delay).as_nanos();
            let iteration = active / self.stride_ns;
            let within = active % self.stride_ns;
            if within >= self.cycle.as_nanos() {
                (TimingPhase::RepeatDelay, iteration, self.cycle)
            } else {
                // within < cycle, so this conversion is representable.
                (TimingPhase::Active, iteration, nanos_to_duration(within))
            }
        };
        let reversed = phase != TimingPhase::Delay && self.timing.direction.reversed(iteration);
        let cycle_position = if reversed {
            self.cycle - progress
        } else {
            progress
        };
        TimingSample {
            phase,
            elapsed,
            total_duration: self.total,
            iteration,
            cycle_position,
            reversed,
        }
    }

    /// Fractional nanoseconds carry across ticks, making a partitioned clock
    /// advance agree exactly with one combined delta at a constant rate.
    pub fn advance(
        &self,
        elapsed: Duration,
        remainder: u32,
        delta: Duration,
    ) -> Result<(Duration, u32), AnimationError> {
        let rate = self.timing.speed;
        // Duration::MAX nanos uses < 94 bits; multiplying by u32 and adding
        // another Duration and remainder stays strictly below u128::MAX.
        let scaled = delta.as_nanos() * u128::from(rate.numerator()) + u128::from(remainder);
        let denominator = u128::from(rate.denominator());
        let nanos = elapsed.as_nanos() + scaled / denominator;
        if let Some(total) = self.total
            && nanos >= total.as_nanos()
        {
            return Ok((total, 0));
        }
        Ok((
            duration_from_nanos(nanos, "animation clock overflow")?,
            (scaled % denominator) as u32,
        ))
    }
}
fn invalid(message: &str) -> AnimationError {
    AnimationError::InvalidInput(message.into())
}
fn duration_from_nanos(nanos: u128, message: &str) -> Result<Duration, AnimationError> {
    if nanos > Duration::MAX.as_nanos() {
        return Err(invalid(message));
    }
    Ok(nanos_to_duration(nanos))
}
fn nanos_to_duration(nanos: u128) -> Duration {
    Duration::new(
        (nanos / 1_000_000_000) as u64,
        (nanos % 1_000_000_000) as u32,
    )
}
