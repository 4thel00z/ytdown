//! indicatif progress bars fed by the library's `Progress` callback, plus a
//! tracing writer that prints log lines above active bars.

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use ytdown::Progress;

/// Create a styled byte-progress bar attached to `mp`.
#[allow(dead_code)] // wired into main in the `get` task
pub fn bar(mp: &MultiProgress, label: &str) -> ProgressBar {
    let pb = mp.add(ProgressBar::new(0));
    pb.set_style(
        ProgressStyle::with_template(
            "{prefix:>6.bold} {bar:30} {percent:>3}% {bytes}/{total_bytes} {bytes_per_sec} eta {eta}",
        )
        .expect("static template"),
    );
    pb.set_prefix(label.to_string());
    pb
}

/// Adapt a bar into the `DownloadBuilder::progress` callback shape.
#[allow(dead_code)] // wired into main in the `get` task
pub fn callback(pb: ProgressBar) -> impl Fn(Progress) + Send + Sync + 'static {
    move |p: Progress| {
        if let Some(total) = p.total_bytes {
            pb.set_length(total);
        }
        pb.set_position(p.bytes_downloaded);
    }
}

/// A `tracing` writer that routes lines through `MultiProgress::println`,
/// so logs print above the bars instead of tearing them.
#[derive(Clone)]
pub struct MpWriter(pub MultiProgress);

impl std::io::Write for MpWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let msg = String::from_utf8_lossy(buf);
        let _ = self.0.println(msg.trim_end_matches('\n'));
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MpWriter {
    type Writer = MpWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Initialize tracing: `-q` off, default `warn`, `-v` info, `-vv`+ debug.
/// `RUST_LOG` overrides when set.
pub fn init_tracing(verbose: u8, quiet: bool, mp: &MultiProgress) {
    let level = if quiet {
        "off"
    } else {
        match verbose {
            0 => "warn",
            1 => "info",
            _ => "debug",
        }
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(MpWriter(mp.clone()))
        .with_ansi(false)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use indicatif::ProgressDrawTarget;

    #[test]
    fn callback_drives_bar_position_and_length() {
        let mp = MultiProgress::with_draw_target(ProgressDrawTarget::hidden());
        let pb = bar(&mp, "test");
        let cb = callback(pb.clone());
        cb(Progress {
            bytes_downloaded: 50,
            total_bytes: Some(200),
            speed_bps: None,
            eta: None,
        });
        assert_eq!(pb.position(), 50);
        assert_eq!(pb.length(), Some(200));
    }
}
