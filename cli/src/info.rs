//! `ytdown info` — emit resolved metadata as JSON on stdout.

use futures::StreamExt;
use serde_json::json;
use ytdown::{MediaInfo, Ytdown};

use crate::app::kind_str;

/// Resolve `url` and print it as JSON. Collections are drained into an
/// `entries` array (bounded by `limit` when given).
pub async fn run(yt: &Ytdown, url: &str, pretty: bool, limit: Option<usize>) -> anyhow::Result<()> {
    let value = match yt.resolve(url).await? {
        MediaInfo::Single(v) => serde_json::to_value(&v)?,
        MediaInfo::Collection(mut col) => {
            let mut entries = Vec::new();
            while let Some(entry) = col.entries.next().await {
                entries.push(serde_json::to_value(&entry?)?);
                if limit.is_some_and(|l| entries.len() >= l) {
                    break;
                }
            }
            json!({
                "id": col.id,
                "title": col.title,
                "kind": kind_str(col.kind),
                "entries": entries,
            })
        }
    };
    let out = if pretty {
        serde_json::to_string_pretty(&value)?
    } else {
        serde_json::to_string(&value)?
    };
    println!("{out}");
    Ok(())
}
