//! A single generation of bounded decoder/audio processes. Replacing a session
//! cancels its readers and kills/reaps all owned children before returning.
use crate::{
    AudioOutput, FfmpegBackend, MediaError, MediaMetadata, PlaybackOptions, VideoFrame,
    process::{self, Process},
};
use std::{
    io::Read,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct Packet {
    bgra: Vec<u8>,
    position: f64,
}
pub(crate) struct DecodeSession {
    video: Option<Process>,
    audio: Option<Process>,
    frames: Receiver<Result<Packet, String>>,
    reader: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    pending: Option<Packet>,
    width: u32,
    height: u32,
    video_finished: bool,
    audio_finished: bool,
    last_frame: Instant,
}
impl DecodeSession {
    pub(crate) fn process_ids(&self) -> Vec<u32> {
        self.video
            .iter()
            .chain(self.audio.iter())
            .map(|process| process.child.id())
            .collect()
    }
    pub(crate) fn start(
        source: &std::path::Path,
        backend: &FfmpegBackend,
        options: &PlaybackOptions,
        metadata: &MediaMetadata,
        quarter_turn: bool,
        position: f64,
        playing: bool,
    ) -> Result<Self, MediaError> {
        let (mut width, mut height) = (metadata.width.unwrap_or(1), metadata.height.unwrap_or(1));
        if quarter_turn {
            // FFmpeg applies display rotation before the scale filter.
            std::mem::swap(&mut width, &mut height);
        }
        let scale = (options.max_width as f64 / width as f64)
            .min(options.max_height as f64 / height as f64)
            .min(1.);
        let width = ((width as f64 * scale).floor() as u32).max(1);
        let height = ((height as f64 * scale).floor() as u32).max(1);
        let (sender, frames) = mpsc::sync_channel(2);
        let cancel = Arc::new(AtomicBool::new(false));
        let mut session = Self {
            video: None,
            audio: None,
            frames,
            reader: None,
            cancel: cancel.clone(),
            pending: None,
            width,
            height,
            video_finished: !metadata.has_video,
            audio_finished: !metadata.has_audio,
            last_frame: Instant::now(),
        };
        if metadata.has_video {
            let mut command = process::command(&backend.tools.ffmpeg);
            command
                .args([
                    "-hide_banner",
                    "-v",
                    "error",
                    "-nostdin",
                    "-max_alloc",
                    "67108864",
                    "-threads",
                    "2",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-format_whitelist",
                    process::FORMAT_WHITELIST,
                    "-ss",
                ])
                .arg(format!("{position:.6}"))
                .arg("-i")
                .arg(source)
                .args(["-map", "0:v:0", "-an", "-sn", "-dn", "-vf"])
                .arg(format!(
                    "scale={width}:{height}:flags=bilinear,fps={}",
                    options.frames_per_second
                ))
                .args(["-threads", "2", "-pix_fmt", "bgra", "-f", "rawvideo"]);
            if !playing {
                command.args(["-frames:v", "1"]);
            }
            command.args(["-blocksize", "65536", "pipe:1"]);
            let mut video = Process::new(process::spawn(&mut command)?);
            let mut stdout = video
                .child
                .stdout
                .take()
                .ok_or_else(|| MediaError::Process("decoder has no stdout".into()))?;
            let fps = options.frames_per_second as f64;
            let bytes_per_frame = width as usize * height as usize * 4;
            session.reader = Some(thread::spawn(move || {
                let mut index = 0;
                loop {
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    let mut bytes = vec![0; bytes_per_frame];
                    match stdout.read_exact(&mut bytes) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
                        Err(error) => {
                            let _ = sender.try_send(Err(error.to_string()));
                            break;
                        }
                    }
                    let mut packet = Ok(Packet {
                        bgra: bytes,
                        position: position + index as f64 / fps,
                    });
                    loop {
                        if cancel.load(Ordering::Acquire) {
                            return;
                        }
                        match sender.try_send(packet) {
                            Ok(()) => break,
                            Err(TrySendError::Full(value)) => {
                                packet = value;
                                thread::sleep(Duration::from_millis(4));
                            }
                            Err(TrySendError::Disconnected(_)) => return,
                        }
                    }
                    index += 1;
                }
            }));
            session.video = Some(video);
        }
        if playing && metadata.has_audio && options.audio == AudioOutput::System {
            let mut command = process::command(&backend.tools.ffplay);
            command
                .stdout(Stdio::null())
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-max_alloc",
                    "67108864",
                    "-protocol_whitelist",
                    "file",
                    "-format_whitelist",
                    process::FORMAT_WHITELIST,
                    "-nodisp",
                    "-vn",
                    "-sn",
                    "-autoexit",
                    "-volume",
                ])
                .arg(format!("{}", (options.volume * 100.).round() as u32))
                .arg("-ss")
                .arg(format!("{position:.6}"))
                .arg("-i")
                .arg(source);
            session.audio = Some(Process::new(process::spawn(&mut command)?));
        } else {
            session.audio_finished = true;
        }
        Ok(session)
    }
    pub(crate) fn frame(
        &mut self,
        position: f64,
        sequence: &mut u64,
    ) -> Result<Option<VideoFrame>, MediaError> {
        let mut latest = None;
        // Decoder is ahead by at most two queued frames plus this pending frame.
        loop {
            if self.pending.is_none() {
                match self.frames.try_recv() {
                    Ok(Ok(packet)) => {
                        self.pending = Some(packet);
                        self.last_frame = Instant::now();
                    }
                    Ok(Err(error)) => return Err(MediaError::Process(error)),
                    Err(_) => break,
                }
            }
            if self
                .pending
                .as_ref()
                .is_some_and(|p| p.position > position + 0.002)
            {
                break;
            }
            if let Some(packet) = self.pending.take() {
                *sequence += 1;
                latest = Some(VideoFrame {
                    width: self.width,
                    height: self.height,
                    bgra: packet.bgra.into(),
                    position: packet.position,
                    sequence: *sequence,
                });
            }
        }
        Ok(latest)
    }
    pub(crate) fn check(&mut self, limits: &crate::MediaLimits) -> Result<(), MediaError> {
        for (process, finished, kind) in [
            (&mut self.video, &mut self.video_finished, "video decoder"),
            (&mut self.audio, &mut self.audio_finished, "audio output"),
        ] {
            if *finished {
                continue;
            }
            if let Some(process) = process {
                match process.child.try_wait() {
                    Ok(Some(status)) => {
                        *finished = true;
                        if !status.success() {
                            return Err(MediaError::Process(format!(
                                "{kind} {status}: {}",
                                process.error_text()
                            )));
                        }
                        // A shorter audio stream can end before its video's duration.
                        // FFplay sometimes returns success after reporting an output error,
                        // so retain its error-level diagnostics instead of claiming success.
                        if kind == "audio output" {
                            let errors = process.error_text();
                            if !errors.is_empty() {
                                return Err(MediaError::Process(errors));
                            }
                        }
                    }
                    Err(error) => return Err(MediaError::Process(error.to_string())),
                    Ok(None) => {}
                }
            }
        }
        if !self.video_finished
            && self.pending.is_none()
            && self.last_frame.elapsed() > limits.decode_stall_timeout
        {
            return Err(MediaError::Timeout(
                "video decoder produced no frames".into(),
            ));
        }
        Ok(())
    }
}
impl Drop for DecodeSession {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(video) = &mut self.video {
            video.stop();
        }
        if let Some(audio) = &mut self.audio {
            audio.stop();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
