//! Runtime-only, elapsed-time image selection shared by native and CPU paint.
//! GPUI owns decoding and supplies complete BGRA frames; encoded loop counts
//! are unavailable, so playback loops indefinitely with a 10ms minimum delay.
use gpui::RenderImage;
use gpui_flow_composite::CompositeError;
use std::{sync::Arc, time::Duration};

pub(crate) const MAX_IMAGE_FRAMES: usize = 4096;
pub(crate) const MAX_TIMELINE_FRAMES: usize = 65_536;
#[derive(Debug)]
pub(crate) struct Timeline {
    ends: Vec<u128>,
    duration: u128,
}
impl Timeline {
    pub(crate) fn new(image: &RenderImage) -> Result<Self, CompositeError> {
        let count = image.frame_count();
        if count == 0 {
            return Err(CompositeError::InvalidImage);
        }
        if count > MAX_IMAGE_FRAMES {
            return Err(CompositeError::InputLimit);
        }
        let mut ends = Vec::new();
        ends.try_reserve_exact(count)
            .map_err(|_| CompositeError::AllocationFailed)?;
        let mut duration = 0u128;
        for frame in 0..count {
            let delay = Duration::from(image.delay(frame))
                .as_nanos()
                .max(10_000_000);
            duration = duration
                .checked_add(delay)
                .ok_or(CompositeError::InputLimit)?;
            ends.push(duration);
        }
        Ok(Self { ends, duration })
    }
    pub(crate) fn frames(&self) -> usize {
        self.ends.len()
    }
    fn index(&self, phase: u128) -> usize {
        self.ends
            .partition_point(|end| *end <= phase % self.duration)
    }
}
#[derive(Clone)]
pub(crate) struct Playback {
    pub image: gpui::ImageId,
    pub timeline: Arc<Timeline>,
    phase: u128,
    anchor: Option<Duration>,
    pub playing: bool,
    pub generation: u64,
}
impl Playback {
    pub(crate) fn new(image: gpui::ImageId, timeline: Arc<Timeline>) -> Self {
        Self {
            image,
            timeline,
            phase: 0,
            anchor: None,
            playing: true,
            generation: 0,
        }
    }
    fn advance(&mut self, now: Duration) {
        if let Some(anchor) = self.anchor {
            // Modulo before addition keeps even Duration::MAX bounded.
            let delta = now.saturating_sub(anchor).as_nanos() % self.timeline.duration;
            self.phase = (self.phase + delta) % self.timeline.duration;
        }
    }
    pub(crate) fn sample(&mut self, now: Duration, visible: bool) -> bool {
        self.advance(now);
        let active = visible && self.playing && self.timeline.frames() > 1;
        self.anchor = active.then_some(now);
        active
    }
    pub(crate) fn index(&self) -> usize {
        self.timeline.index(self.phase)
    }
    pub(crate) fn set_playing(&mut self, now: Duration, playing: bool) {
        if self.playing != playing {
            self.advance(now);
            self.anchor = None;
            self.playing = playing;
            self.generation = self.generation.wrapping_add(1);
        }
    }
    pub(crate) fn seek(&mut self, time: Duration) {
        self.phase = time.as_nanos() % self.timeline.duration;
        self.anchor = None;
        self.generation = self.generation.wrapping_add(1);
    }
    pub(crate) fn freeze(&mut self, now: Duration) {
        self.advance(now);
        self.anchor = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn image(delays: &[u32]) -> RenderImage {
        RenderImage::new(
            delays
                .iter()
                .map(|delay| {
                    image::Frame::from_parts(
                        image::RgbaImage::new(1, 1),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(*delay, 1),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }
    fn playback(delays: &[u32]) -> Playback {
        let image = image(delays);
        Playback::new(image.id, Arc::new(Timeline::new(&image).unwrap()))
    }
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }
    #[test]
    fn unequal_delays_exact_boundaries_and_large_elapsed_loop() {
        let mut p = playback(&[20, 70, 10]);
        p.sample(ms(0), true);
        for (time, frame) in [(19, 0), (20, 1), (89, 1), (90, 2), (100, 0), (123_490, 2)] {
            p.sample(ms(time), true);
            assert_eq!(p.index(), frame);
        }
        p.sample(Duration::MAX, true);
        assert!(p.index() < 3);
    }
    #[test]
    fn pause_hidden_resume_preserve_partial_frame_and_seek_invalidates() {
        let mut p = playback(&[20, 70]);
        p.sample(ms(0), true);
        p.set_playing(ms(15), false);
        p.sample(ms(900), true);
        assert_eq!(p.index(), 0);
        p.set_playing(ms(1000), true);
        p.sample(ms(1000), true);
        p.sample(ms(1005), true);
        assert_eq!(p.index(), 1);
        p.sample(ms(1010), false);
        p.sample(ms(9000), false);
        p.sample(ms(10000), true);
        p.sample(ms(10064), true);
        assert_eq!(p.index(), 1);
        p.sample(ms(10065), true);
        assert_eq!(p.index(), 0);
        let generation = p.generation;
        p.seek(ms(20));
        assert_eq!(p.index(), 1);
        assert_ne!(p.generation, generation);
        p.seek(Duration::ZERO);
        assert_eq!(p.index(), 0);
    }
    #[test]
    fn frame_limits_zero_delay_and_independent_instances() {
        assert!(Timeline::new(&image(&[])).is_err());
        assert!(Timeline::new(&image(&vec![1; MAX_IMAGE_FRAMES + 1])).is_err());
        let mut a = playback(&[0, 1]);
        let mut b = a.clone();
        a.sample(ms(0), true);
        a.sample(ms(10), true);
        b.sample(ms(10), true);
        assert_eq!((a.index(), b.index()), (1, 0));
        let mut static_image = playback(&[0]);
        assert!(!static_image.sample(ms(0), true));
    }
    #[test]
    fn original_gif_decodes_distinct_full_frames_delays_and_transparent_disposal() {
        use image::AnimationDecoder;
        let bytes = include_bytes!("../tests/fixtures/composite-animation.gif");
        let frames = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))
            .unwrap()
            .into_frames()
            .collect_frames()
            .unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(Duration::from(frames[0].delay()), ms(200));
        assert_eq!(Duration::from(frames[1].delay()), ms(400));
        assert_eq!(Duration::from(frames[2].delay()), ms(600));
        assert_eq!(frames[0].buffer().get_pixel(80, 10).0, [237, 72, 72, 255]);
        assert_eq!(frames[1].buffer().get_pixel(80, 10).0[3], 0);
        assert_eq!(frames[2].buffer().get_pixel(10, 10).0[3], 0);
        assert_eq!(frames[2].buffer().get_pixel(80, 10).0, [71, 124, 232, 255]);
        assert!(frames.iter().all(|f| f.buffer().dimensions() == (96, 64)));
    }
}
