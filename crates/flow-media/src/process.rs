use crate::MediaError;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
pub(crate) const FORMAT_WHITELIST: &str =
    "mov,matroska,webm,avi,wav,flac,mp3,ogg,aac,flv,mpeg,mpegts";
pub(crate) const ERROR_BYTES: usize = 16 * 1024;

pub(crate) fn command(program: &std::path::Path) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW; audio still uses SDL normally.
    }
    command
}
pub(crate) fn spawn(command: &mut Command) -> Result<Child, MediaError> {
    command.spawn().map_err(|error| {
        MediaError::ToolUnavailable(format!(
            "{}: {error}",
            command.get_program().to_string_lossy()
        ))
    })
}
pub(crate) fn reader(
    mut stream: impl Read + Send + 'static,
    limit: usize,
) -> JoinHandle<(Vec<u8>, bool)> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut overflow = false;
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let remaining = limit.saturating_sub(bytes.len());
                    bytes.extend_from_slice(&chunk[..n.min(remaining)]);
                    overflow |= n > remaining;
                }
            }
        }
        (bytes, overflow)
    })
}
/// Owns and reaps its child even on early returns. Readers are always drained,
/// bounded in memory and joined after the process has closed its pipes.
pub(crate) struct Process {
    pub child: Child,
    pub errors: Option<JoinHandle<(Vec<u8>, bool)>>,
}
impl Process {
    pub fn new(mut child: Child) -> Self {
        let errors = child
            .stderr
            .take()
            .map(|stream| reader(stream, ERROR_BYTES));
        Self { child, errors }
    }
    pub fn error_text(&mut self) -> String {
        self.errors
            .take()
            .and_then(|thread| thread.join().ok())
            .map(|(bytes, _)| String::from_utf8_lossy(&bytes).trim().to_owned())
            .unwrap_or_default()
    }
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.errors.take() {
            let _ = reader.join();
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn capture(
    command: &mut Command,
    timeout: Duration,
    cancel: &Arc<AtomicBool>,
    max_bytes: usize,
) -> Result<Vec<u8>, MediaError> {
    capture_observed(command, timeout, cancel, max_bytes, |_| {})
}
pub(crate) fn capture_observed(
    command: &mut Command,
    timeout: Duration,
    cancel: &Arc<AtomicBool>,
    max_bytes: usize,
    started: impl FnOnce(u32),
) -> Result<Vec<u8>, MediaError> {
    if cancel.load(Ordering::Acquire) {
        return Err(MediaError::Cancelled);
    }
    let mut process = Process::new(spawn(command)?);
    started(process.child.id());
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or_else(|| MediaError::Process("missing stdout pipe".into()))?;
    let output = reader(stdout, max_bytes);
    let start = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Acquire) {
            process.stop();
            let _ = output.join();
            return Err(MediaError::Cancelled);
        }
        if start.elapsed() >= timeout {
            process.stop();
            let _ = output.join();
            return Err(MediaError::Timeout("subprocess exceeded deadline".into()));
        }
        match process.child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                process.stop();
                let _ = output.join();
                return Err(MediaError::Process(error.to_string()));
            }
        }
    };
    let (bytes, overflow) = output
        .join()
        .map_err(|_| MediaError::Process("stdout reader stopped".into()))?;
    let errors = process.error_text();
    if !status.success() {
        return Err(MediaError::Process(format!("{status}: {errors}")));
    }
    if overflow {
        return Err(MediaError::ResourceLimit(
            "subprocess output exceeded bounded capture".into(),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slow_ffmpeg() -> Command {
        let mut command = command(std::path::Path::new("ffmpeg"));
        command.args([
            "-nostdin",
            "-hide_banner",
            "-v",
            "error",
            "-re",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=16x16:rate=1:duration=30",
            "-f",
            "null",
            "-",
        ]);
        command
    }

    #[test]
    #[ignore = "requires ffmpeg; validates process deadline and cancellation"]
    fn capture_timeout_and_cancel_are_bounded() {
        let cancel = Arc::new(AtomicBool::new(false));
        let start = Instant::now();
        let error = capture(
            &mut slow_ffmpeg(),
            Duration::from_millis(150),
            &cancel,
            1024,
        )
        .unwrap_err();
        assert!(matches!(error, MediaError::Timeout(_)));
        assert!(start.elapsed() < Duration::from_secs(2));
        let signal = cancel.clone();
        let trigger = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            signal.store(true, Ordering::Release);
        });
        let start = Instant::now();
        let error =
            capture(&mut slow_ffmpeg(), Duration::from_secs(10), &cancel, 1024).unwrap_err();
        assert_eq!(error, MediaError::Cancelled);
        assert!(start.elapsed() < Duration::from_secs(2));
        trigger.join().unwrap();
    }
}
