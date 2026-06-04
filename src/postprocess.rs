//! Postprocessing of downloaded media via ffmpeg.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Muxes split A/V via the system ffmpeg binary.
pub struct FfmpegMerger {
    binary: PathBuf,
}

impl Default for FfmpegMerger {
    fn default() -> Self {
        Self::new()
    }
}

impl FfmpegMerger {
    /// Creates a merger that invokes `ffmpeg` from `PATH`.
    pub fn new() -> Self {
        Self {
            binary: "ffmpeg".into(),
        }
    }

    /// Creates a merger that invokes the ffmpeg binary at the given path.
    pub fn with_binary(p: impl Into<PathBuf>) -> Self {
        Self { binary: p.into() }
    }

    /// Muxes a split `video` and `audio` stream into `out` without re-encoding.
    ///
    /// Runs `ffmpeg -y -i video -i audio -c copy out`. A non-zero exit status
    /// is mapped to [`Error::Postprocess`] carrying the tail of stderr.
    pub async fn merge(&self, video: &Path, audio: &Path, out: &Path) -> Result<()> {
        self.run(&[
            "-y".as_ref(),
            "-i".as_ref(),
            video.as_os_str(),
            "-i".as_ref(),
            audio.as_os_str(),
            "-c".as_ref(),
            "copy".as_ref(),
            out.as_os_str(),
        ])
        .await
    }

    /// Transcodes `input` into `out`, choosing codecs from the target container.
    ///
    /// Runs `ffmpeg -y -i input <codec args> out`. A non-zero exit status is
    /// mapped to [`Error::Postprocess`] carrying the tail of stderr.
    pub async fn convert(&self, input: &Path, out: &Path) -> Result<()> {
        let mut args: Vec<&std::ffi::OsStr> = vec!["-y".as_ref(), "-i".as_ref(), input.as_os_str()];
        for a in codec_args_for(out) {
            args.push(a.as_ref());
        }
        args.push(out.as_os_str());
        self.run(&args).await
    }

    async fn run(&self, args: &[&std::ffi::OsStr]) -> Result<()> {
        let output = tokio::process::Command::new(&self.binary)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                Error::Postprocess(format!(
                    "failed to spawn ffmpeg ({}): {e}",
                    self.binary.display()
                ))
            })?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(Error::Postprocess(format!(
                "ffmpeg exited with {}: {}",
                output.status,
                last_lines(&stderr, 5)
            )))
        }
    }
}

/// Returns the codec arguments appropriate for the output container.
fn codec_args_for(out: &Path) -> &'static [&'static str] {
    match out
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4") | Some("m4a") | Some("mov") => &["-c:v", "libx264", "-c:a", "aac"],
        Some("webm") => &["-c:v", "libvpx-vp9", "-c:a", "libopus"],
        Some("mp3") => &["-c:a", "libmp3lame"],
        Some("opus") => &["-c:a", "libopus"],
        // Unknown container: let ffmpeg pick defaults for the muxer.
        _ => &[],
    }
}

/// Returns the last `n` non-empty lines of `s`, joined with `\n`.
fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Writes an executable shell script acting as a fake ffmpeg. The script
    /// records every argument (one per line) to `record` and, when
    /// `make_output` is set, creates the file named by its last argument so
    /// callers can observe success. Returns the script path.
    fn fake_ffmpeg(
        dir: &std::path::Path,
        record: &std::path::Path,
        exit_code: i32,
        make_output: bool,
        stderr: &str,
    ) -> PathBuf {
        let script = dir.join("fake-ffmpeg.sh");
        let mut f = std::fs::File::create(&script).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, ": > '{}'", record.display()).unwrap();
        writeln!(
            f,
            "for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done",
            record.display()
        )
        .unwrap();
        if !stderr.is_empty() {
            writeln!(f, "printf '%s\\n' '{stderr}' 1>&2").unwrap();
        }
        if make_output {
            // The output path is always the final positional argument.
            writeln!(f, "out=").unwrap();
            writeln!(f, "for a in \"$@\"; do out=\"$a\"; done").unwrap();
            writeln!(f, ": > \"$out\"").unwrap();
        }
        writeln!(f, "exit {exit_code}").unwrap();
        drop(f);
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        script
    }

    #[tokio::test]
    async fn merge_invokes_ffmpeg_with_expected_args() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("args.txt");
        let script = fake_ffmpeg(dir.path(), &record, 0, true, "");

        let video = dir.path().join("v.mp4");
        let audio = dir.path().join("a.m4a");
        let out = dir.path().join("out.mp4");
        std::fs::write(&video, b"video").unwrap();
        std::fs::write(&audio, b"audio").unwrap();

        let merger = FfmpegMerger::with_binary(&script);
        merger.merge(&video, &audio, &out).await.unwrap();

        let args = std::fs::read_to_string(&record).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        // -i <video> -i <audio> -c copy <out>
        assert!(lines.contains(&"-i"));
        assert!(lines.contains(&video.to_str().unwrap()));
        assert!(lines.contains(&audio.to_str().unwrap()));
        assert!(lines.contains(&"-c"));
        assert!(lines.contains(&"copy"));
        assert!(lines.contains(&out.to_str().unwrap()));
        // -i appears exactly twice, once per input.
        let i_count = lines.iter().filter(|l| **l == "-i").count();
        assert_eq!(i_count, 2);
        assert!(out.exists());
    }

    #[tokio::test]
    async fn convert_uses_container_codec_args() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("args.txt");
        let script = fake_ffmpeg(dir.path(), &record, 0, true, "");

        let input = dir.path().join("in.webm");
        let out = dir.path().join("out.mp4");
        std::fs::write(&input, b"data").unwrap();

        let merger = FfmpegMerger::with_binary(&script);
        merger.convert(&input, &out).await.unwrap();

        let args = std::fs::read_to_string(&record).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        assert!(lines.contains(&"-c:v"));
        assert!(lines.contains(&"libx264"));
        assert!(lines.contains(&"aac"));
        assert!(out.exists());
    }

    #[tokio::test]
    async fn missing_binary_is_postprocess_error() {
        let dir = tempfile::tempdir().unwrap();
        let merger = FfmpegMerger::with_binary(dir.path().join("does-not-exist-ffmpeg"));
        let err = merger
            .merge(
                std::path::Path::new("v.mp4"),
                std::path::Path::new("a.m4a"),
                std::path::Path::new("o.mp4"),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Postprocess(_)));
    }

    #[tokio::test]
    async fn nonzero_exit_includes_stderr_tail() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("args.txt");
        let script = fake_ffmpeg(dir.path(), &record, 1, false, "boom: muxing failed");

        let merger = FfmpegMerger::with_binary(&script);
        let err = merger
            .merge(
                &dir.path().join("v.mp4"),
                &dir.path().join("a.m4a"),
                &dir.path().join("o.mp4"),
            )
            .await
            .unwrap_err();
        match err {
            Error::Postprocess(msg) => assert!(msg.contains("boom: muxing failed")),
            other => panic!("expected Postprocess, got {other:?}"),
        }
    }

    #[test]
    fn last_lines_keeps_tail() {
        let s = "a\n\nb\nc\nd\ne\nf\n";
        assert_eq!(last_lines(s, 3), "d\ne\nf");
    }
}
