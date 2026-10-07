//! Bounded local audio overviews, decoded without opening an audio device.
use crate::{FfmpegTools, MediaError, MediaLimits, probe, process};
use std::{
    fs::Metadata,
    io::Read,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

const BINS: usize = 256;
const SAMPLE_RATE: u64 = 48_000;
const MAX_DURATION: Duration = Duration::from_secs(30 * 60);
const TIMEOUT: Duration = Duration::from_secs(15);
// Container duration can include codec delay/padding absent from decoded PCM.
const PADDING_FRAMES: u64 = SAMPLE_RATE / 4;

/// A whole-file overview, not sample-accurate editing data.
#[derive(Clone, Debug)]
pub struct AudioWaveform {
    /// The finite duration reported by the local file's metadata probe.
    pub duration_seconds: f64,
    /// 256 equal time bins of absolute peak amplitude, clamped to 0..=1.
    /// Silence remains zero. Stereo uses the larger channel magnitude, so
    /// opposite-phase channels never disappear through a mono downmix.
    pub peaks: Vec<f32>,
}

pub(crate) fn extract(
    source: &Path,
    tools: &FfmpegTools,
    limits: &MediaLimits,
    cancel: &Arc<AtomicBool>,
) -> Result<AudioWaveform, MediaError> {
    let start = Instant::now();
    check_cancel(cancel)?;
    let mut bounded = limits.clone();
    bounded.max_duration = bounded.max_duration.min(MAX_DURATION);
    bounded.probe_timeout = bounded.probe_timeout.min(TIMEOUT);
    let canonical = probe::source(source, &bounded)?;
    let identity = SourceIdentity::read(&canonical)?;
    let metadata = probe::probe(&canonical, tools, &bounded, cancel)?;
    identity.check(source, &canonical, &bounded)?;
    check_cancel(cancel)?;
    if !metadata.has_audio {
        return Err(MediaError::InvalidMedia(
            "waveform requires an audio stream".into(),
        ));
    }
    let channels = match metadata.audio_channels {
        Some(channels @ 1..=2) => channels,
        _ => {
            return Err(MediaError::InvalidMedia(
                "waveform supports only mono or stereo audio".into(),
            ));
        }
    };
    if metadata.audio_sample_rate.is_none_or(|rate| rate == 0) {
        return Err(MediaError::InvalidMedia(
            "waveform requires a valid audio sample rate".into(),
        ));
    }
    let remaining = TIMEOUT
        .checked_sub(start.elapsed())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(timeout)?;
    let accumulator = PcmAccumulator::new(metadata.duration, channels);
    let result = decode(&canonical, tools, accumulator, remaining, cancel);
    // Recheck the caller's path as well as file identity: this also catches a
    // symlink retargeted while the canonical file was being decoded.
    identity.check(source, &canonical, &bounded)?;
    check_cancel(cancel)?;
    Ok(AudioWaveform {
        duration_seconds: metadata.duration,
        peaks: result?,
    })
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), MediaError> {
    if cancel.load(Ordering::Acquire) {
        Err(MediaError::Cancelled)
    } else {
        Ok(())
    }
}

fn timeout() -> MediaError {
    MediaError::Timeout("waveform probe/decode exceeded 15-second deadline".into())
}

#[derive(PartialEq, Eq)]
struct SourceIdentity {
    bytes: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    unix: (u64, u64, i64, i64),
}
impl SourceIdentity {
    fn read(path: &Path) -> Result<Self, MediaError> {
        let metadata = path
            .metadata()
            .map_err(|error| MediaError::InvalidSource(error.to_string()))?;
        if !metadata.is_file() {
            return Err(MediaError::InvalidSource(
                "waveform source is no longer a regular file".into(),
            ));
        }
        Self::from_metadata(&metadata)
    }

    fn from_metadata(metadata: &Metadata) -> Result<Self, MediaError> {
        Ok(Self {
            bytes: metadata.len(),
            modified: metadata
                .modified()
                .map_err(|error| MediaError::InvalidSource(error.to_string()))?,
            created: metadata.created().ok(),
            #[cfg(unix)]
            unix: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        })
    }

    fn check(
        &self,
        requested: &Path,
        canonical: &Path,
        limits: &MediaLimits,
    ) -> Result<(), MediaError> {
        if probe::source(requested, limits)? != canonical || *self != Self::read(canonical)? {
            return Err(MediaError::InvalidSource(
                "waveform source changed during extraction".into(),
            ));
        }
        Ok(())
    }
}

// Ownership deliberately mirrors process::capture, but stdout is reduced into
// fixed bins instead of retaining PCM (up to ~660 MiB for a 30-minute stereo
// file). Drop always kills/reaps the child before joining both pipe readers.
struct Decode {
    process: process::Process,
    reader: Option<JoinHandle<Result<Vec<f32>, MediaError>>>,
}
impl Drop for Decode {
    fn drop(&mut self) {
        self.process.stop();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn decode(
    source: &Path,
    tools: &FfmpegTools,
    accumulator: PcmAccumulator,
    deadline: Duration,
    cancel: &Arc<AtomicBool>,
) -> Result<Vec<f32>, MediaError> {
    check_cancel(cancel)?;
    let start = Instant::now();
    let mut command = process::command(&tools.ffmpeg);
    command.args([
        "-nostdin",
        "-hide_banner",
        "-v",
        "error",
        "-xerror",
        "-max_alloc",
        "67108864",
        "-threads",
        "1",
        "-filter_threads",
        "1",
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        process::FORMAT_WHITELIST,
        "-probesize",
        "5242880",
        "-analyzeduration",
        "5000000",
        "-err_detect",
        "explode",
        "-i",
    ]);
    command.arg(source).args([
        "-map",
        "0:a:0",
        "-vn",
        "-sn",
        "-dn",
        "-threads",
        "1",
        "-acodec",
        "pcm_f32le",
        "-ar",
        "48000",
        "-ac",
    ]);
    command
        .arg(accumulator.channels.to_string())
        .arg("-t")
        // Allow one frame beyond the reader's ceiling so oversized output is
        // rejected, rather than silently accepted after FFmpeg truncates it.
        .arg(format!(
            "{:.9}",
            (accumulator.max_frames + 1) as f64 / SAMPLE_RATE as f64
        ))
        .args(["-f", "f32le", "pipe:1"]);
    let mut job = Decode {
        process: process::Process::new(process::spawn(&mut command)?),
        reader: None,
    };
    let stdout = job
        .process
        .child
        .stdout
        .take()
        .ok_or_else(|| MediaError::Process("missing waveform stdout pipe".into()))?;
    let signal = cancel.clone();
    job.reader = Some(
        thread::Builder::new()
            .name("flow-waveform-pcm".into())
            .spawn(move || read_pcm(stdout, accumulator, &signal))
            .map_err(|error| MediaError::Process(error.to_string()))?,
    );
    let mut peaks = None;
    let status = loop {
        check_cancel(cancel)?;
        if start.elapsed() >= deadline {
            return Err(timeout());
        }
        if job
            .reader
            .as_ref()
            .is_some_and(|reader| reader.is_finished())
        {
            peaks = Some(join_reader(job.reader.take().unwrap())?);
        }
        match job.process.child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(MediaError::Process(error.to_string())),
        }
    };
    let peaks = match peaks {
        Some(peaks) => peaks,
        None => join_reader(job.reader.take().unwrap())?,
    };
    let errors = job.process.error_text();
    if !status.success() || !errors.is_empty() {
        return Err(MediaError::InvalidMedia(format!(
            "waveform decoder {status}: {errors}"
        )));
    }
    check_cancel(cancel)?;
    if start.elapsed() >= deadline {
        return Err(timeout());
    }
    Ok(peaks)
}

fn join_reader(reader: JoinHandle<Result<Vec<f32>, MediaError>>) -> Result<Vec<f32>, MediaError> {
    reader
        .join()
        .map_err(|_| MediaError::Process("waveform PCM reader stopped".into()))?
}

fn read_pcm(
    mut stream: impl Read,
    mut accumulator: PcmAccumulator,
    cancel: &AtomicBool,
) -> Result<Vec<f32>, MediaError> {
    let mut bytes = [0u8; 8192];
    loop {
        check_cancel(cancel)?;
        match stream.read(&mut bytes) {
            Ok(0) => return accumulator.finish(),
            Ok(count) => accumulator.push(&bytes[..count])?,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(MediaError::Process(error.to_string())),
        }
    }
}

struct PcmAccumulator {
    peaks: [f32; BINS],
    expected_frames: u64,
    min_frames: u64,
    max_frames: u64,
    frames: u64,
    channels: u32,
    channel: u32,
    frame_peak: f32,
    pending: [u8; 4],
    pending_len: usize,
}
impl PcmAccumulator {
    // Called only after probe validates finite, positive, <=30-minute duration
    // and mono/stereo channels, keeping all arithmetic below 87 million frames.
    fn new(duration: f64, channels: u32) -> Self {
        let expected_frames = (duration * SAMPLE_RATE as f64).ceil() as u64;
        Self {
            peaks: [0.; BINS],
            expected_frames,
            // Permit codec padding, but never accept an almost-empty decode of
            // a short file. Complete PCM frames alone do not prove completeness.
            min_frames: expected_frames
                .saturating_sub(PADDING_FRAMES)
                .max(expected_frames.div_ceil(2)),
            max_frames: expected_frames + PADDING_FRAMES,
            frames: 0,
            channels,
            channel: 0,
            frame_peak: 0.,
            pending: [0; 4],
            pending_len: 0,
        }
    }

    fn push(&mut self, mut bytes: &[u8]) -> Result<(), MediaError> {
        while !bytes.is_empty() {
            let count = bytes.len().min(4 - self.pending_len);
            self.pending[self.pending_len..self.pending_len + count]
                .copy_from_slice(&bytes[..count]);
            self.pending_len += count;
            bytes = &bytes[count..];
            if self.pending_len < 4 {
                continue;
            }
            self.pending_len = 0;
            let sample = f32::from_le_bytes(self.pending);
            if !sample.is_finite() {
                return Err(MediaError::InvalidMedia(
                    "waveform PCM contains a non-finite sample".into(),
                ));
            }
            if self.frames >= self.max_frames {
                return Err(MediaError::ResourceLimit(
                    "waveform PCM exceeds probed duration and padding limit".into(),
                ));
            }
            self.frame_peak = self.frame_peak.max(sample.abs().min(1.));
            self.channel += 1;
            if self.channel == self.channels {
                let bin = ((self.frames * BINS as u64) / self.expected_frames)
                    .min((BINS - 1) as u64) as usize;
                self.peaks[bin] = self.peaks[bin].max(self.frame_peak);
                self.frames += 1;
                self.channel = 0;
                self.frame_peak = 0.;
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<Vec<f32>, MediaError> {
        if self.pending_len != 0 || self.channel != 0 {
            return Err(MediaError::InvalidMedia(
                "waveform PCM ends with a truncated sample or channel frame".into(),
            ));
        }
        if self.frames == 0 || self.frames < self.min_frames {
            return Err(MediaError::InvalidMedia(
                "waveform PCM is empty or shorter than the probed duration".into(),
            ));
        }
        Ok(self.peaks.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn mono_peaks_follow_time_bins_and_clamp_without_normalizing() {
        let mut accumulator = PcmAccumulator::new(4. / SAMPLE_RATE as f64, 1);
        accumulator.push(&pcm(&[0.1, -0.5, 0., 1.5])).unwrap();
        let peaks = accumulator.finish().unwrap();
        assert_eq!(peaks.len(), BINS);
        assert_eq!(peaks[0], 0.1);
        assert_eq!(peaks[64], 0.5);
        assert_eq!(peaks[128], 0.);
        assert_eq!(peaks[192], 1.);
        assert_eq!(peaks.iter().filter(|peak| **peak != 0.).count(), 3);
    }

    #[test]
    fn stereo_phase_and_arbitrary_pipe_chunks_preserve_channel_peaks() {
        let bytes = pcm(&[0.75, -0.75, -0.25, 0.25]);
        let mut accumulator = PcmAccumulator::new(2. / SAMPLE_RATE as f64, 2);
        for chunk in bytes.chunks(3) {
            accumulator.push(chunk).unwrap();
        }
        let peaks = accumulator.finish().unwrap();
        assert_eq!(peaks[0], 0.75);
        assert_eq!(peaks[128], 0.25);
    }

    #[test]
    fn silence_stays_silent_and_corrupt_pcm_is_rejected() {
        let mut silent = PcmAccumulator::new(1. / SAMPLE_RATE as f64, 1);
        silent.push(&pcm(&[0.])).unwrap();
        assert!(silent.finish().unwrap().iter().all(|peak| *peak == 0.));
        for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut bad = PcmAccumulator::new(1., 1);
            assert!(matches!(
                bad.push(&pcm(&[sample])),
                Err(MediaError::InvalidMedia(_))
            ));
        }
        for (channels, bytes) in [(1, vec![0u8; 3]), (2, pcm(&[0.2]))] {
            let mut bad = PcmAccumulator::new(1., channels);
            bad.push(&bytes).unwrap();
            assert!(matches!(bad.finish(), Err(MediaError::InvalidMedia(_))));
        }
    }

    #[test]
    fn empty_short_and_excessive_decodes_are_rejected() {
        assert!(PcmAccumulator::new(1., 1).finish().is_err());
        let mut short = PcmAccumulator::new(1., 1);
        short.push(&pcm(&[0.1; 64])).unwrap();
        assert!(matches!(short.finish(), Err(MediaError::InvalidMedia(_))));
        let mut excessive = PcmAccumulator::new(1. / SAMPLE_RATE as f64, 1);
        for _ in 0..excessive.max_frames {
            excessive.push(&pcm(&[0.])).unwrap();
        }
        assert!(matches!(
            excessive.push(&pcm(&[0.])),
            Err(MediaError::ResourceLimit(_))
        ));
    }

    #[test]
    #[ignore = "requires host-installed ffmpeg and ffprobe; no audio device"]
    fn local_stereo_fixture_extracts_real_peaks_and_duration() {
        use std::{fs, io::Write};
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(
            std::env::temp_dir().join(format!("flow-waveform-{}-{unique}", std::process::id())),
        );
        fs::create_dir(&fixture.0).unwrap();
        let path = fixture.0.join("opposite-phase.wav");
        let data_bytes = SAMPLE_RATE as u32 * 4;
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&(SAMPLE_RATE as u32).to_le_bytes()).unwrap();
        file.write_all(&(SAMPLE_RATE as u32 * 4).to_le_bytes())
            .unwrap();
        file.write_all(&4u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        for frame in 0..SAMPLE_RATE {
            let sample: i16 = if frame < SAMPLE_RATE / 2 { 8192 } else { 24576 };
            file.write_all(&sample.to_le_bytes()).unwrap();
            file.write_all(&(-sample).to_le_bytes()).unwrap();
        }
        drop(file);
        let before = fs::read(&path).unwrap();
        let backend = crate::FfmpegBackend::default();
        let waveform = backend
            .waveform(&path, &Arc::new(AtomicBool::new(false)))
            .unwrap();
        assert!((waveform.duration_seconds - 1.).abs() < 0.001);
        assert_eq!(waveform.peaks.len(), BINS);
        assert!(
            waveform.peaks[..128]
                .iter()
                .all(|peak| (*peak - 0.25).abs() < 0.001)
        );
        assert!(
            waveform.peaks[128..]
                .iter()
                .all(|peak| (*peak - 0.75).abs() < 0.001)
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            backend
                .waveform(&path, &Arc::new(AtomicBool::new(true)))
                .unwrap_err(),
            MediaError::Cancelled
        );
        let limited = crate::FfmpegBackend {
            limits: MediaLimits {
                max_duration: Duration::from_millis(500),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(matches!(
            limited.waveform(&path, &Arc::new(AtomicBool::new(false))),
            Err(MediaError::ResourceLimit(_))
        ));
    }
}
