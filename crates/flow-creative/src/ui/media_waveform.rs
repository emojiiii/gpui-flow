//! One waveform decoder across live workbenches. Pending work stays in the
//! existing four media-view slots; there is no growing queue of blocked jobs.
use super::*;
use gpui_flow_media::{AudioWaveform, MediaError};
use std::sync::atomic::{AtomicBool, Ordering};

static DECODING: AtomicBool = AtomicBool::new(false);

pub(super) struct DecodePermit;
impl Drop for DecodePermit {
    fn drop(&mut self) {
        DECODING.store(false, Ordering::Release);
    }
}

enum State {
    Pending,
    Loading,
    Ready(Arc<AudioWaveform>),
    Failed(String),
}

pub(super) struct WaveformLoad {
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
    state: State,
}
impl WaveformLoad {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            cancelled: Arc::new(AtomicBool::new(false)),
            state: State::Pending,
        }
    }
    pub(super) fn try_start(&mut self) -> Option<(PathBuf, Arc<AtomicBool>, DecodePermit)> {
        if !matches!(self.state, State::Pending)
            || self.cancelled.load(Ordering::Acquire)
            || DECODING
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return None;
        }
        self.state = State::Loading;
        Some((self.path.clone(), self.cancelled.clone(), DecodePermit))
    }
    pub(super) fn complete(&mut self, result: Result<AudioWaveform, MediaError>) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        self.state = match result {
            Ok(waveform) => State::Ready(Arc::new(waveform)),
            Err(error) => State::Failed(error.to_string()),
        };
    }
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub(super) fn render(&self, position: f64) -> AnyElement {
        match &self.state {
            State::Ready(waveform) => waveform_canvas(waveform.clone(), position),
            State::Pending | State::Loading => {
                label(tr("正在读取真实音频波形…", "Reading audio waveform…").into())
            }
            State::Failed(error) => label(tr_format!(
                "波形不可用：{error}",
                "Waveform unavailable: {error}"
            )),
        }
    }
}
impl Drop for WaveformLoad {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn label(text: String) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .text_color(rgb(0x8c7963))
        .child(text)
        .into_any_element()
}

fn waveform_canvas(waveform: Arc<AudioWaveform>, position: f64) -> AnyElement {
    let progress = progress(position, waveform.duration_seconds);
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let width: f32 = bounds.size.width.into();
            let height: f32 = bounds.size.height.into();
            if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
                return;
            }
            let count = waveform
                .peaks
                .len()
                .min((width / 3.).floor().max(1.) as usize);
            if count == 0 {
                return;
            }
            let step = width / count as f32;
            for bar in 0..count {
                let start = bar * waveform.peaks.len() / count;
                let end = ((bar + 1) * waveform.peaks.len() / count).max(start + 1);
                let peak = waveform.peaks[start..end]
                    .iter()
                    .copied()
                    .fold(0., f32::max)
                    .clamp(0., 1.);
                let bar_height = (peak * height).max(1.);
                let rect = Bounds::new(
                    bounds.origin + point(px(bar as f32 * step), px((height - bar_height) / 2.)),
                    size(px((step - 1.).max(1.)), px(bar_height)),
                );
                window.paint_quad(fill(
                    rect,
                    rgb(if bar as f64 / (count as f64) < progress {
                        0xa86f2c
                    } else {
                        0xd3b58f
                    }),
                ));
            }
            if progress > 0. {
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.origin
                            + point(px((width * progress as f32).min(width - 1.)), px(0.)),
                        size(px(1.), px(height)),
                    ),
                    rgb(0x80541f),
                ));
            }
        },
    )
    .size_full()
    .into_any_element()
}

pub(super) fn progress(position: f64, duration: f64) -> f64 {
    if !position.is_finite() || !duration.is_finite() || duration <= 0. {
        return 0.;
    }
    (position / duration).clamp(0., 1.)
}

pub(super) fn seek_fraction(x: f32, origin: f32, width: f32) -> Option<f64> {
    if !x.is_finite() || !origin.is_finite() || !width.is_finite() || width <= 0. {
        return None;
    }
    Some(f64::from(((x - origin) / width).clamp(0., 1.)))
}

#[cfg(test)]
mod tests {
    use super::{progress, seek_fraction};
    #[test]
    fn timeline_maps_screen_coordinates_and_clamps_finite_playback() {
        assert_eq!(seek_fraction(350., 200., 300.), Some(0.5));
        assert_eq!(seek_fraction(0., 200., 300.), Some(0.));
        assert_eq!(seek_fraction(700., 200., 300.), Some(1.));
        assert_eq!(seek_fraction(250., 200., 100.), Some(0.5));
        assert_eq!(seek_fraction(250., 200., 0.), None);
        assert_eq!(seek_fraction(f32::NAN, 200., 100.), None);
        assert_eq!(progress(9., 6.), 1.);
        assert_eq!(progress(3., 6.), 0.5);
        assert_eq!(progress(3., f64::NAN), 0.);
    }
}
