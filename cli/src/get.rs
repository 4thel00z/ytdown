//! `ytdown get` — download media.

use std::path::{Path, PathBuf};

use clap::Args;
use indicatif::MultiProgress;
use ytdown::postprocess::FfmpegMerger;
use ytdown::{Container, Format, MediaInfo, VideoInfo, Ytdown};

use crate::selector::{self, FormatSpec, Selection};
use crate::template::{RenderCtx, Template};
use crate::{app, progress};

/// Arguments to `ytdown get`.
#[derive(Args)]
pub struct GetArgs {
    /// URL of a video, playlist, channel, or ytsearch: query.
    pub url: String,

    /// Format: best | bestvideo | bestaudio | ITAG | VIDEO_ITAG+AUDIO_ITAG.
    #[arg(short = 'f', long = "format")]
    pub format: Option<FormatSpec>,

    /// Only consider formats up to this height.
    #[arg(long)]
    pub max_height: Option<u32>,

    /// Only consider formats in this container.
    #[arg(long, value_enum)]
    pub container: Option<ContainerArg>,

    /// Output path template ({title} {id} {ext} {height} {itag} {uploader} {index}).
    #[arg(short, long, default_value = "{title}.{ext}")]
    pub output: Template,

    /// Stop after this many collection entries.
    #[arg(long)]
    pub limit: Option<usize>,

    /// Skip this many collection entries first.
    #[arg(long, default_value_t = 0)]
    pub skip: usize,

    /// Parallel range-chunk connections per download.
    #[arg(long, default_value_t = 1)]
    pub concurrency: usize,

    /// Chunk size in bytes for parallel downloads.
    #[arg(long)]
    pub chunk_size: Option<u64>,

    /// Max retry attempts per request.
    #[arg(long)]
    pub retries: Option<u32>,

    /// Do not resume partial files.
    #[arg(long)]
    pub no_resume: bool,

    /// Path to the ffmpeg binary.
    #[arg(long, default_value = "ffmpeg")]
    pub ffmpeg: PathBuf,

    /// Never open the interactive picker.
    #[arg(long)]
    pub no_tui: bool,
}

/// `--container` values, mapped onto the library's `Container`.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum ContainerArg {
    /// MPEG-4.
    Mp4,
    /// WebM.
    Webm,
}

impl From<ContainerArg> for Container {
    fn from(c: ContainerArg) -> Self {
        match c {
            ContainerArg::Mp4 => Container::Mp4,
            ContainerArg::Webm => Container::WebM,
        }
    }
}

/// Entry point for `ytdown get`.
pub async fn run(yt: &Ytdown, mp: &MultiProgress, args: &GetArgs) -> anyhow::Result<()> {
    // Validate flag combinations before any network traffic.
    if matches!(
        args.format,
        Some(FormatSpec::Itag(_)) | Some(FormatSpec::Merge(..))
    ) && (args.max_height.is_some() || args.container.is_some())
    {
        return Err(selector::itag_filter_conflict().into());
    }
    let ffmpeg_ok = app::ffmpeg_available(&args.ffmpeg).await;
    match yt.resolve(&args.url).await? {
        MediaInfo::Single(video) => download_video(yt, mp, &video, args, ffmpeg_ok, None).await,
        MediaInfo::Collection(_) => {
            anyhow::bail!("collection downloads land in the next commit") // replaced in Task 12
        }
    }
}

/// Select a format and download one video. `index` is the 1-based position
/// within a collection (`None` for a directly-requested video).
async fn download_video(
    yt: &Ytdown,
    mp: &MultiProgress,
    video: &VideoInfo,
    args: &GetArgs,
    ffmpeg_ok: bool,
    index: Option<usize>,
) -> anyhow::Result<()> {
    let container = args.container.map(Container::from);
    let selection = selector::resolve(
        args.format.as_ref(),
        &video.formats,
        args.max_height,
        container.as_ref(),
        ffmpeg_ok,
    )?;
    download_selection(yt, mp, video, selection, args, ffmpeg_ok, index).await
}

async fn download_selection(
    yt: &Ytdown,
    mp: &MultiProgress,
    video: &VideoInfo,
    selection: Selection<'_>,
    args: &GetArgs,
    ffmpeg_ok: bool,
    index: Option<usize>,
) -> anyhow::Result<()> {
    let dest = dest_for(video, &selection, &args.output, index);
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }
    match selection {
        Selection::Single(f) => {
            let pb = progress::bar(mp, "get");
            apply(yt.download(f, &dest), args)
                .progress(progress::callback(pb.clone()))
                .await?;
            pb.finish();
        }
        Selection::Merged {
            video: vf,
            audio: af,
        } => {
            anyhow::ensure!(
                ffmpeg_ok,
                "merging split streams requires ffmpeg ({}); install it, pass --ffmpeg, or use -f best",
                args.ffmpeg.display()
            );
            let vtmp = part_path(&dest, "video.part");
            let atmp = part_path(&dest, "audio.part");
            let vpb = progress::bar(mp, "video");
            apply(yt.download(vf, &vtmp), args)
                .progress(progress::callback(vpb.clone()))
                .await?;
            vpb.finish();
            let apb = progress::bar(mp, "audio");
            apply(yt.download(af, &atmp), args)
                .progress(progress::callback(apb.clone()))
                .await?;
            apb.finish();
            FfmpegMerger::with_binary(&args.ffmpeg)
                .merge(&vtmp, &atmp, &dest)
                .await?;
            let _ = tokio::fs::remove_file(&vtmp).await;
            let _ = tokio::fs::remove_file(&atmp).await;
        }
    }
    eprintln!("saved {}", dest.display());
    Ok(())
}

/// Thread the download tuning flags onto a builder.
fn apply<'a>(b: ytdown::DownloadBuilder<'a>, args: &GetArgs) -> ytdown::DownloadBuilder<'a> {
    let mut b = b.concurrency(args.concurrency).resume(!args.no_resume);
    if let Some(c) = args.chunk_size {
        b = b.chunk_size(c);
    }
    if let Some(r) = args.retries {
        b = b.retries(r);
    }
    b
}

/// Render the output template for this selection.
fn dest_for(
    video: &VideoInfo,
    selection: &Selection<'_>,
    template: &Template,
    index: Option<usize>,
) -> PathBuf {
    let (ext, height, itag) = match selection {
        Selection::Single(f) => (ext_for(f), f.video.as_ref().and_then(|v| v.height), f.itag),
        Selection::Merged { video: vf, .. } => (
            "mp4".to_string(),
            vf.video.as_ref().and_then(|v| v.height),
            vf.itag,
        ),
    };
    template.render(&RenderCtx {
        title: &video.title,
        id: &video.id,
        ext: &ext,
        height,
        itag,
        uploader: video.uploader.as_deref(),
        index,
    })
}

/// File extension for a single format.
fn ext_for(f: &Format) -> String {
    match f.container.as_ref() {
        Some(Container::Mp4) => "mp4".into(),
        Some(Container::WebM) => "webm".into(),
        Some(Container::M4a) => "m4a".into(),
        Some(Container::Weba) => "weba".into(),
        Some(Container::Other(s)) => s.clone(),
        _ => "bin".into(),
    }
}

/// Hidden temp sibling next to `dest` (mirrors the lib's convention).
fn part_path(dest: &Path, suffix: &str) -> PathBuf {
    let stem = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("ytdown");
    let name = format!(".{stem}.{suffix}");
    match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(name),
        _ => PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_path_is_hidden_sibling() {
        assert_eq!(
            part_path(Path::new("out/v.mp4"), "video.part"),
            PathBuf::from("out/.v.mp4.video.part")
        );
        assert_eq!(
            part_path(Path::new("v.mp4"), "audio.part"),
            PathBuf::from(".v.mp4.audio.part")
        );
    }

    #[test]
    fn ext_for_maps_containers() {
        let mut f = Format::default();
        f.container = Some(Container::WebM);
        assert_eq!(ext_for(&f), "webm");
        assert_eq!(ext_for(&Format::default()), "bin");
    }
}
