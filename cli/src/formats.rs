//! `ytdown formats` — list a video's available formats.

use ytdown::{MediaInfo, Ytdown};

use crate::table;

/// Resolve `url` and print its formats as a table (or JSON).
pub async fn run(yt: &Ytdown, url: &str, json: bool) -> anyhow::Result<()> {
    match yt.resolve(url).await? {
        MediaInfo::Single(v) => {
            if json {
                println!("{}", serde_json::to_string(&v.formats)?);
            } else {
                println!("{}", table::formats_table(&v.formats));
            }
            Ok(())
        }
        MediaInfo::Collection(_) => anyhow::bail!(
            "`formats` expects a single video URL; this resolved to a collection — pass one of its entries (see `ytdown info`)"
        ),
    }
}
