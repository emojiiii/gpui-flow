use crate::{AnimationError, AnimationSpec, Easing, MAX_TWEENS_PER_ANIMATION, Track, tween};
use gpui_flow_engine::{ChangeOrigin, ElementHandle, SceneEngine};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Duration};

pub const MAX_SEQUENCE_KEYFRAMES: usize = 64;

/// Explicit, positive-duration target keyframes. The first segment starts at the
/// current pose; each following segment starts at the previous cumulative pose.
/// A property omitted from a segment holds its last value. There is no implicit
/// zero-duration first pose, delay, repeat, reverse, or nested timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationSequence {
    pub keyframes: Vec<AnimationSpec>,
}
impl AnimationSequence {
    pub fn new(keyframes: Vec<AnimationSpec>) -> Self {
        Self { keyframes }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Segment {
    pub start: Duration,
    pub end: Duration,
    pub easing: Easing,
}

pub(crate) fn prepare(
    engine: &SceneEngine,
    target: ElementHandle,
    sequence: AnimationSequence,
) -> Result<(Vec<Segment>, Vec<Track>, Duration), AnimationError> {
    let invalid = |s: &str| AnimationError::InvalidInput(s.into());
    if sequence.keyframes.is_empty() || sequence.keyframes.len() > MAX_SEQUENCE_KEYFRAMES {
        return Err(invalid("1..=64 keyframes required"));
    }
    let mut properties = BTreeSet::new();
    let mut total_ms = 0u64;
    let mut segments = Vec::with_capacity(sequence.keyframes.len());
    for frame in &sequence.keyframes {
        if frame.duration_ms == 0
            || frame.tweens.is_empty()
            || frame.tweens.len() > MAX_TWEENS_PER_ANIMATION
        {
            return Err(invalid(
                "positive duration and 1..=32 tweens per keyframe required",
            ));
        }
        let mut unique = BTreeSet::new();
        for tween in &frame.tweens {
            if !unique.insert(tween.property.clone()) {
                return Err(invalid("duplicate property in keyframe"));
            }
            properties.insert(tween.property.clone());
        }
        let start = Duration::from_millis(total_ms);
        total_ms = total_ms
            .checked_add(frame.duration_ms)
            .ok_or_else(|| invalid("sequence duration overflow"))?;
        segments.push(Segment {
            start,
            end: Duration::from_millis(total_ms),
            easing: frame.easing,
        });
    }
    if properties.len() > MAX_TWEENS_PER_ANIMATION {
        return Err(invalid("at most 32 distinct properties per sequence"));
    }
    let mut tracks = Vec::with_capacity(properties.len());
    for property in properties {
        let from = tween::read(engine, target, &property)?;
        tracks.push(Track {
            property,
            values: vec![from.clone()],
            last: from,
        });
    }
    for frame in sequence.keyframes {
        let mut endpoint = Vec::with_capacity(tracks.len());
        for track in &mut tracks {
            let from = track.values.last().expect("initial pose exists");
            let to = frame
                .tweens
                .iter()
                .find(|t| t.property == track.property)
                .map_or_else(|| from.clone(), |t| t.to.clone());
            tween::validate_pair(&track.property, from, &to)?;
            let (key, value) = tween::update(engine, target, &track.property, &to)?;
            endpoint.push((target, key, value));
            track.values.push(to);
        }
        engine.validate_properties_batch_with_origin(ChangeOrigin::Animation, endpoint)?;
    }
    Ok((segments, tracks, Duration::from_millis(total_ms)))
}
