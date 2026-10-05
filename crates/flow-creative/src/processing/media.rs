use super::*;
use gpui_flow_media::{FfmpegTools, MediaMetadata};
use serde_json::Value;
use std::{
    io::Write,
    process::{Child, Command, Stdio},
    sync::Arc,
    thread::{self, JoinHandle},
};

const FORMAT_WHITELIST: &str = "mov,matroska,webm,wav,flac,mp3,ogg";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROBE_BYTES: usize = 1024 * 1024;
const MAX_ERROR_BYTES: usize = 16 * 1024;
const MAX_VIDEO_PIXELS: u64 = 4096 * 4096;

struct Probe {
    metadata: MediaMetadata,
    video_codec: Option<String>,
    audio_codecs: Vec<String>,
}

pub(super) fn execute(
    settings: &ProcessingSettings,
    source: &Path,
    asset: &ProjectAsset,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<ProcessingOutput, ProcessingError> {
    let start = Instant::now();
    let tools = FfmpegTools::default();
    let demuxer = source_format(source, &asset.media_type)?;
    let before = probe(source, demuxer, &tools, cancel)?;
    match settings {
        ProcessingSettings::Video { .. } => {
            if !before.metadata.has_video
                || before.metadata.width.unwrap_or(0) < 2
                || before.metadata.height.unwrap_or(0) < 2
            {
                return Err(ProcessingError::InvalidSource(
                    "video processor requires a real video stream at least 2×2 pixels".into(),
                ));
            }
            if (before.metadata.width, before.metadata.height)
                != (Some(asset.width), Some(asset.height))
            {
                return Err(ProcessingError::InvalidSource(
                    "video dimensions no longer match the project asset".into(),
                ));
            }
        }
        ProcessingSettings::Audio { format, .. } => {
            if !before.metadata.has_audio {
                return Err(ProcessingError::InvalidSource(
                    "audio processor requires a real audio stream".into(),
                ));
            }
            if *format == AudioOutputFormat::Mp3 && before.metadata.audio_channels.unwrap_or(0) > 2
            {
                return Err(ProcessingError::Unsupported("MP3 supports mono/stereo input here; use WAV to preserve a multichannel source".into()));
            }
        }
        _ => {
            return Err(ProcessingError::InvalidSettings(
                "expected video or audio settings".into(),
            ));
        }
    }
    check_deadline(cancel, start, PROCESS_MEDIA_TIMEOUT)?;
    // Only this module creates the destination. FFmpeg writes encoded bytes to
    // a pipe, never opens a caller-supplied output path, and also uses -n.
    let mut output = OutputGuard::create(destination)?;
    let file = output.file.as_ref().expect("new output").try_clone()?;
    let mut command = command(&tools.ffmpeg);
    command.args([
        "-nostdin",
        "-hide_banner",
        "-v",
        "error",
        "-n",
        "-max_alloc",
        "67108864",
        "-cpucount",
        "1",
        "-filter_threads",
        "1",
        "-filter_complex_threads",
        "1",
        "-threads",
        "1",
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        FORMAT_WHITELIST,
    ]);
    input_options(&mut command, demuxer);
    command.arg("-i").arg(source);
    command.args(["-map_metadata", "-1", "-map_chapters", "-1", "-sn", "-dn"]);
    match *settings {
        ProcessingSettings::Video { crf, max_edge } => {
            command
                .args([
                    "-map", "0:v:0", "-map", "0:a?", "-c:v", "libx264", "-preset", "veryfast",
                    "-crf",
                ])
                .arg(crf.to_string());
            // Generated from validated integers, never a document-supplied
            // expression. H.264 4:2:0 requires even dimensions; rounding down
            // may change aspect by less than two pixels and never upscales.
            command.arg("-vf").arg(format!(
                "scale=w='min(iw,{max_edge})':h='min(ih,{max_edge})':force_original_aspect_ratio=decrease:force_divisible_by=2"
            ));
            command.args([
                "-pix_fmt",
                "yuv420p",
                "-threads:v",
                "1",
                "-x264-params",
                "threads=1:lookahead_threads=1",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-ar",
                "48000",
                "-threads:a",
                "1",
                "-max_muxing_queue_size",
                "1024",
                "-f",
                "mp4",
                // A fragmented MP4 is a real seekable local MP4 after it has
                // been written; this muxer mode also works on a bounded pipe.
                "-movflags",
                "+frag_keyframe+empty_moov+default_base_moof",
            ]);
        }
        ProcessingSettings::Audio {
            format,
            bitrate_kbps,
            sample_rate,
        } => {
            command
                .args(["-map", "0:a:0", "-vn", "-ar"])
                .arg(sample_rate.to_string());
            command.args(["-threads:a", "1"]);
            match format {
                AudioOutputFormat::Wav => {
                    command.args(["-c:a", "pcm_s16le", "-f", "wav"]);
                }
                AudioOutputFormat::Mp3 => {
                    command
                        .args(["-c:a", "libmp3lame", "-b:a"])
                        .arg(format!("{bitrate_kbps}k"));
                    command.args(["-f", "mp3"]);
                }
            }
        }
        _ => unreachable!("validated media settings"),
    }
    command.args(["-protocol_whitelist", "pipe", "pipe:1"]);
    let remaining = PROCESS_MEDIA_TIMEOUT.saturating_sub(start.elapsed());
    run(
        &mut command,
        Output::File(file, MAX_PROCESS_MEDIA_BYTES),
        remaining,
        cancel,
    )?;
    output
        .file
        .as_mut()
        .expect("output remains open")
        .sync_all()?;
    check_deadline(cancel, start, PROCESS_MEDIA_TIMEOUT)?;
    // Verify the actual encoded output, not just the input's probe metadata.
    // The child must have exited successfully and written nonempty bytes first.
    let output_metadata = fs::metadata(destination)?;
    if output_metadata.len() == 0 || output_metadata.len() > MAX_PROCESS_MEDIA_BYTES {
        return Err(ProcessingError::InvalidOutput(
            "transcoder produced an empty or oversized output".into(),
        ));
    }
    let after_demuxer = match settings {
        ProcessingSettings::Video { .. } => "mov",
        ProcessingSettings::Audio {
            format: AudioOutputFormat::Wav,
            ..
        } => "wav",
        ProcessingSettings::Audio {
            format: AudioOutputFormat::Mp3,
            ..
        } => "mp3",
        _ => unreachable!(),
    };
    let after = probe(destination, after_demuxer, &tools, cancel)?;
    validate_output(settings, &before, &after)?;
    check_deadline(cancel, start, PROCESS_MEDIA_TIMEOUT)?;
    let dimensions = if settings.kind() == ProcessingKind::Video {
        (
            after.metadata.width.unwrap_or(0),
            after.metadata.height.unwrap_or(0),
        )
    } else {
        (0, 0)
    };
    output.finish(settings, dimensions, MAX_PROCESS_MEDIA_BYTES, cancel)
}

fn validate_output(
    settings: &ProcessingSettings,
    before: &Probe,
    after: &Probe,
) -> Result<(), ProcessingError> {
    // Encoder priming and audio padding may differ by a few frames. Larger
    // losses are errors, including files truncated by resource limits.
    let tolerance = 0.5;
    if (before.metadata.duration - after.metadata.duration).abs() > tolerance {
        return Err(ProcessingError::InvalidOutput(
            "transcoded duration does not match the complete source".into(),
        ));
    }
    match *settings {
        ProcessingSettings::Video { max_edge, .. } => {
            let width = after.metadata.width.unwrap_or(0);
            let height = after.metadata.height.unwrap_or(0);
            if after.video_codec.as_deref() != Some("h264")
                || width < 2
                || height < 2
                || width.max(height) > max_edge
                || !width.is_multiple_of(2)
                || !height.is_multiple_of(2)
                || u64::from(width) * u64::from(height)
                    > u64::from(before.metadata.width.unwrap_or(0))
                        * u64::from(before.metadata.height.unwrap_or(0))
                || after.audio_codecs.len() != before.audio_codecs.len()
                || after.audio_codecs.iter().any(|codec| codec != "aac")
            {
                return Err(ProcessingError::InvalidOutput(
                    "output must be bounded H.264 MP4 retaining every source audio track as AAC"
                        .into(),
                ));
            }
        }
        ProcessingSettings::Audio {
            format,
            sample_rate,
            ..
        } => {
            let codec = match format {
                AudioOutputFormat::Wav => "pcm_s16le",
                AudioOutputFormat::Mp3 => "mp3",
            };
            if after.metadata.has_video
                || after.audio_codecs.len() != 1
                || after.audio_codecs[0] != codec
                || after.metadata.audio_sample_rate != Some(sample_rate)
                || after.metadata.audio_channels != before.metadata.audio_channels
            {
                return Err(ProcessingError::InvalidOutput("output codec, sample rate, or audio channels do not match the requested conversion".into()));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn source_format(source: &Path, media_type: &str) -> Result<&'static str, ProcessingError> {
    let file = File::open(source)?;
    if !file.metadata()?.is_file() {
        return Err(ProcessingError::InvalidSource(
            "only regular local media is accepted".into(),
        ));
    }
    let mut header = Vec::new();
    file.take(32).read_to_end(&mut header)?;
    let is_wave = header.len() >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WAVE";
    let is_mp4 = header.len() >= 16 && &header[4..8] == b"ftyp";
    let is_mp3 = header.starts_with(b"ID3")
        || (header.len() >= 2 && header[0] == 0xff && header[1] & 0xe0 == 0xe0);
    match media_type {
        "video/mp4" | "audio/mp4" if is_mp4 => Ok("mov"),
        "video/webm" if header.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) => Ok("matroska"),
        "audio/wav" if is_wave => Ok("wav"),
        "audio/mpeg" if is_mp3 => Ok("mp3"),
        "audio/flac" if header.starts_with(b"fLaC") => Ok("flac"),
        "audio/ogg" if header.starts_with(b"OggS") => Ok("ogg"),
        _ => Err(ProcessingError::InvalidSource("media header does not match its supported local container; URLs and playlists are rejected".into())),
    }
}

fn input_options(command: &mut Command, demuxer: &str) {
    command.args(["-f", demuxer]);
    if demuxer == "mov" {
        // MOV may contain data-reference atoms. Never follow external files,
        // including absolute local references embedded in an imported movie.
        command.args(["-enable_drefs", "0", "-use_absolute_path", "0"]);
    }
}

fn probe(
    path: &Path,
    demuxer: &str,
    tools: &FfmpegTools,
    cancel: &AtomicBool,
) -> Result<Probe, ProcessingError> {
    let mut command = command(&tools.ffprobe);
    command.args([
        "-v", "error", "-max_alloc", "67108864", "-cpucount", "1",
        "-threads", "1", "-protocol_whitelist", "file", "-format_whitelist", FORMAT_WHITELIST,
        "-show_entries", "format=format_name,duration:stream=codec_type,codec_name,width,height,sample_rate,channels,duration",
        "-of", "json",
    ]);
    input_options(&mut command, demuxer);
    command.arg(path);
    let bytes = run(
        &mut command,
        Output::Capture(MAX_PROBE_BYTES),
        PROBE_TIMEOUT,
        cancel,
    )?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        ProcessingError::InvalidSource(format!("invalid FFprobe response: {error}"))
    })?;
    parse_probe(&value, demuxer)
}

fn parse_probe(value: &Value, demuxer: &str) -> Result<Probe, ProcessingError> {
    let streams = value["streams"]
        .as_array()
        .filter(|streams| !streams.is_empty() && streams.len() <= 16)
        .ok_or_else(|| ProcessingError::ResourceLimit("media must have 1–16 streams".into()))?;
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"].as_str() == Some("video"));
    let audio: Vec<_> = streams
        .iter()
        .filter(|stream| stream["codec_type"].as_str() == Some("audio"))
        .collect();
    if audio.len() > 8 || (video.is_none() && audio.is_empty()) {
        return Err(ProcessingError::ResourceLimit(
            "media must have video/audio and at most eight audio tracks".into(),
        ));
    }
    let number = |value: &Value| value.as_f64().or_else(|| value.as_str()?.parse().ok());
    let duration = number(&value["format"]["duration"])
        .or_else(|| {
            streams
                .iter()
                .filter_map(|stream| number(&stream["duration"]))
                .max_by(f64::total_cmp)
        })
        .filter(|duration| {
            duration.is_finite() && *duration > 0. && *duration <= MAX_PROCESS_MEDIA_SECONDS as f64
        })
        .ok_or_else(|| {
            ProcessingError::ResourceLimit(
                "media needs a finite duration no longer than 30 minutes".into(),
            )
        })?;
    let dimension = |stream: &Value, name: &str| {
        stream[name]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
    };
    for stream in streams
        .iter()
        .filter(|stream| stream["codec_type"].as_str() == Some("video"))
    {
        let width = dimension(stream, "width").unwrap_or(0);
        let height = dimension(stream, "height").unwrap_or(0);
        if width == 0
            || height == 0
            || width > 16384
            || height > 16384
            || u64::from(width) * u64::from(height) > MAX_VIDEO_PIXELS
        {
            return Err(ProcessingError::ResourceLimit(
                "video dimensions are missing or exceed the 16 MP processing limit".into(),
            ));
        }
    }
    for stream in &audio {
        let channels = dimension(stream, "channels").unwrap_or(0);
        let rate = number(&stream["sample_rate"]).unwrap_or(0.);
        if !(1..=8).contains(&channels)
            || !rate.is_finite()
            || !(8000. ..=192000.).contains(&rate)
            || rate.fract() != 0.
        {
            return Err(ProcessingError::ResourceLimit(
                "audio must have 1–8 channels and a finite 8–192 kHz sample rate".into(),
            ));
        }
    }
    let format = value["format"]["format_name"].as_str().unwrap_or("");
    if !format
        .split(',')
        .any(|name| name == demuxer || (demuxer == "matroska" && name == "webm"))
    {
        return Err(ProcessingError::InvalidSource(
            "decoded container does not match the local media header".into(),
        ));
    }
    let first_audio = audio.first().copied();
    let metadata = MediaMetadata {
        duration,
        width: video.and_then(|stream| dimension(stream, "width")),
        height: video.and_then(|stream| dimension(stream, "height")),
        has_video: video.is_some(),
        has_audio: !audio.is_empty(),
        audio_sample_rate: first_audio
            .and_then(|stream| number(&stream["sample_rate"]))
            .map(|rate| rate as u32),
        audio_channels: first_audio.and_then(|stream| dimension(stream, "channels")),
        format: format.into(),
    };
    Ok(Probe {
        metadata,
        video_codec: video
            .and_then(|stream| stream["codec_name"].as_str())
            .map(str::to_owned),
        audio_codecs: audio
            .iter()
            .map(|stream| {
                stream["codec_name"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned()
            })
            .collect(),
    })
}

fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

enum Output {
    Capture(usize),
    File(File, u64),
}

struct Running {
    child: Child,
    stderr: Option<JoinHandle<Vec<u8>>>,
    stdout: Option<JoinHandle<Result<Vec<u8>, ProcessingError>>>,
}

impl Running {
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    fn join(&mut self) {
        if let Some(stdout) = self.stdout.take() {
            let _ = stdout.join();
        }
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
        self.join();
    }
}

fn run(
    command: &mut Command,
    output: Output,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, ProcessingError> {
    check_cancel(cancel)?;
    if timeout.is_zero() {
        return Err(ProcessingError::Timeout);
    }
    let child = command.spawn().map_err(|error| ProcessingError::ToolUnavailable(format!("{} is unavailable: {error}; install FFmpeg with your operating system's package manager", command.get_program().to_string_lossy())))?;
    let mut process = Running {
        child,
        stderr: None,
        stdout: None,
    };
    let mut stderr = process
        .child
        .stderr
        .take()
        .ok_or_else(|| ProcessingError::Process("missing FFmpeg stderr pipe".into()))?;
    process.stderr = Some(thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stderr.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    let keep = count.min(MAX_ERROR_BYTES.saturating_sub(bytes.len()));
                    bytes.extend_from_slice(&chunk[..keep]);
                    // Always drain additional stderr: full pipes must never
                    // deadlock cancellation or a completed encoder.
                }
            }
        }
        bytes
    }));
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or_else(|| ProcessingError::Process("missing FFmpeg stdout pipe".into()))?;
    let failed = Arc::new(AtomicBool::new(false));
    let reader_failed = Arc::clone(&failed);
    process.stdout = Some(thread::spawn(move || {
        let result = copy_output(stdout, output);
        if result.is_err() {
            reader_failed.store(true, Ordering::Release);
        }
        result
    }));
    let start = Instant::now();
    let status = loop {
        check_deadline(cancel, start, timeout)?;
        if failed.load(Ordering::Acquire) {
            process.stop();
            return process
                .stdout
                .take()
                .expect("stdout reader")
                .join()
                .map_err(|_| ProcessingError::Process("output reader panicked".into()))?;
        }
        match process.child.try_wait()? {
            Some(status) => break status,
            None => thread::sleep(Duration::from_millis(10)),
        }
    };
    let bytes = process
        .stdout
        .take()
        .expect("stdout reader")
        .join()
        .map_err(|_| ProcessingError::Process("output reader panicked".into()))??;
    let errors = process
        .stderr
        .take()
        .expect("stderr reader")
        .join()
        .map_err(|_| ProcessingError::Process("error reader panicked".into()))?;
    check_cancel(cancel)?;
    if !status.success() {
        let errors: String = String::from_utf8_lossy(&errors)
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(4096)
            .collect();
        return Err(ProcessingError::Process(format!(
            "FFmpeg process failed ({status}): {}",
            errors.trim()
        )));
    }
    Ok(bytes)
}

fn copy_output(mut reader: impl Read, mut output: Output) -> Result<Vec<u8>, ProcessingError> {
    let limit = match output {
        Output::Capture(limit) => limit as u64,
        Output::File(_, limit) => limit,
    };
    let mut bytes = Vec::new();
    let mut written = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if count as u64 > limit.saturating_sub(written) {
            return Err(ProcessingError::ResourceLimit(
                "processor output exceeded its byte budget; partial output was discarded".into(),
            ));
        }
        match &mut output {
            Output::Capture(_) => bytes.extend_from_slice(&chunk[..count]),
            Output::File(file, _) => file.write_all(&chunk[..count])?,
        }
        written += count as u64;
    }
    if let Output::File(file, _) = &mut output {
        file.flush()?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::tests::TestDirectory;
    use serde_json::json;

    #[test]
    fn local_container_checks_reject_playlists_and_type_confusion() {
        let directory = TestDirectory::new();
        let source = directory.0.join("source.mp4");
        for bytes in [
            b"#EXTM3U\nfile:///etc/passwd".as_slice(),
            b"https://example.com/video.mp4",
            b"<MPD></MPD>",
        ] {
            fs::write(&source, bytes).unwrap();
            assert!(source_format(&source, "video/mp4").is_err());
        }
        fs::write(&source, b"RIFF\0\0\0\0WAVE").unwrap();
        assert_eq!(source_format(&source, "audio/wav").unwrap(), "wav");
        assert!(source_format(&source, "video/mp4").is_err());
    }

    #[test]
    fn probe_and_output_checks_reject_oversize_and_missing_audio() {
        let value = json!({"format":{"format_name":"mov,mp4","duration":"2"}, "streams":[{"codec_type":"video","codec_name":"h264","width":16,"height":16},{"codec_type":"audio","codec_name":"aac","channels":2,"sample_rate":"48000"}]});
        let before = parse_probe(&value, "mov").unwrap();
        let mut changed = value.clone();
        changed["streams"].as_array_mut().unwrap().pop();
        let after = parse_probe(&changed, "mov").unwrap();
        assert!(
            validate_output(&ProcessingKind::Video.default_settings(), &before, &after).is_err()
        );
        for duration in ["NaN", "inf", "0", "1801"] {
            let mut changed = value.clone();
            changed["format"]["duration"] = json!(duration);
            assert!(parse_probe(&changed, "mov").is_err());
        }
        let mut changed = value;
        changed["streams"][0]["width"] = json!(u32::MAX);
        assert!(parse_probe(&changed, "mov").is_err());
    }

    #[test]
    fn bounded_output_reader_never_writes_past_limit() {
        let directory = TestDirectory::new();
        let path = directory.0.join("output");
        let output = OutputGuard::create(&path).unwrap();
        let file = output.file.as_ref().unwrap().try_clone().unwrap();
        assert!(copy_output(&b"12345678"[..], Output::File(file, 4)).is_err());
        assert!(fs::metadata(&path).unwrap().len() <= 4);
        drop(output);
        assert!(!path.exists());
    }

    fn fixture_wav(path: &Path) {
        // One second of real PCM, built without external fixture downloads.
        let samples = 8000u32;
        let data_length = samples * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_length.to_le_bytes());
        for index in 0..samples {
            bytes.extend_from_slice(&((index % 80) as i16 * 100 - 4000).to_le_bytes());
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    #[ignore = "requires installed ffmpeg and ffprobe; executes real WAV/MP3 conversion"]
    fn ffmpeg_fixture_transcodes_real_audio_and_validates_output() {
        let _gate = PROCESS_GATE.lock().unwrap();
        let directory = TestDirectory::new();
        let source = directory.0.join("source.wav");
        fixture_wav(&source);
        let asset = ProjectAsset {
            key: "assets/source.wav".into(),
            name: "source".into(),
            media_type: "audio/wav".into(),
            byte_len: fs::metadata(&source).unwrap().len(),
            width: 0,
            height: 0,
        };
        for format in [AudioOutputFormat::Wav, AudioOutputFormat::Mp3] {
            let settings = ProcessingSettings::Audio {
                format,
                bitrate_kbps: 128,
                sample_rate: 44100,
            };
            let destination = directory.0.join(format!("output.{}", settings.extension()));
            let result = execute(
                &settings,
                &source,
                &asset,
                &destination,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert_eq!(result.media_type, settings.media_type());
            assert_eq!((result.width, result.height), (0, 0));
            assert!(result.byte_len > 100);
        }
    }

    #[test]
    #[ignore = "requires installed ffmpeg/ffprobe with libx264; validates actual video transcoding and all audio tracks"]
    fn ffmpeg_fixture_transcodes_video_and_preserves_two_audio_tracks() {
        let _gate = PROCESS_GATE.lock().unwrap();
        let directory = TestDirectory::new();
        let source = directory.0.join("source.mp4");
        let mut fixture = command(Path::new("ffmpeg"));
        fixture
            .args([
                "-nostdin",
                "-hide_banner",
                "-v",
                "error",
                "-n",
                "-cpucount",
                "1",
                "-filter_threads",
                "1",
                "-filter_complex_threads",
                "1",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=32x16:r=10:d=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:duration=1",
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:a",
                "-c:v",
                "libx264",
                "-threads:v",
                "1",
                "-c:a",
                "aac",
                "-threads:a",
                "1",
                "-f",
                "mp4",
            ])
            .arg(&source);
        run(
            &mut fixture,
            Output::Capture(1024),
            Duration::from_secs(20),
            &AtomicBool::new(false),
        )
        .unwrap();
        let source_bytes = fs::read(&source).unwrap();
        let asset = ProjectAsset {
            key: "assets/source.mp4".into(),
            name: "source".into(),
            media_type: "video/mp4".into(),
            byte_len: source_bytes.len() as u64,
            width: 32,
            height: 16,
        };
        let settings = ProcessingSettings::Video {
            crf: 23,
            max_edge: 16,
        };
        let output = execute(
            &settings,
            &source,
            &asset,
            &directory.0.join("output.mp4"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!((output.width, output.height), (16, 8));
        assert!(output.byte_len > 100);
        let after = probe(
            &output.path,
            "mov",
            &FfmpegTools::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(after.video_codec.as_deref(), Some("h264"));
        assert_eq!(after.audio_codecs, vec!["aac".to_owned(), "aac".to_owned()]);
        assert_eq!(fs::read(source).unwrap(), source_bytes);
    }

    #[test]
    #[ignore = "requires installed ffmpeg; validates timeout/cancellation cleanup and child reaping"]
    fn ffmpeg_process_timeout_and_cancel_remove_incomplete_output() {
        let _gate = PROCESS_GATE.lock().unwrap();
        let directory = TestDirectory::new();
        for cancelled in [false, true] {
            let destination = directory.0.join(format!("cancel-{cancelled}.wav"));
            let output = OutputGuard::create(&destination).unwrap();
            let file = output.file.as_ref().unwrap().try_clone().unwrap();
            let mut command = command(Path::new("ffmpeg"));
            command.args([
                "-nostdin",
                "-hide_banner",
                "-v",
                "error",
                "-re",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=30",
                "-threads",
                "1",
                "-f",
                "wav",
                "pipe:1",
            ]);
            let cancel = Arc::new(AtomicBool::new(false));
            let signal = Arc::clone(&cancel);
            let worker = if cancelled {
                Some(thread::spawn(move || {
                    thread::sleep(Duration::from_millis(100));
                    signal.store(true, Ordering::Release);
                }))
            } else {
                None
            };
            let result = run(
                &mut command,
                Output::File(file, 1024 * 1024),
                Duration::from_millis(if cancelled { 5000 } else { 100 }),
                &cancel,
            );
            if let Some(worker) = worker {
                worker.join().unwrap();
            }
            assert!(matches!(
                result,
                Err(ProcessingError::Cancelled) | Err(ProcessingError::Timeout)
            ));
            drop(output);
            assert!(!destination.exists());
        }
    }
}
