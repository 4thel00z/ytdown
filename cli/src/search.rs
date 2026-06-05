//! `ytdown search` — list results for a ytsearch: query.

use futures::StreamExt;
use ytdown::{MediaInfo, Ytdown};

use crate::table;

/// Search and print up to `n` results as a table (or JSON).
pub async fn run(yt: &Ytdown, query: &str, n: usize, json: bool) -> anyhow::Result<()> {
    let info = yt.resolve(&format!("ytsearch:{query}")).await?;
    let mut col = match info {
        MediaInfo::Collection(c) => c,
        MediaInfo::Single(_) => anyhow::bail!("search unexpectedly resolved to a single video"),
    };
    let mut entries = Vec::new();
    while let Some(e) = col.entries.next().await {
        entries.push(e?);
        if entries.len() >= n {
            break;
        }
    }
    if json {
        println!("{}", serde_json::to_string(&entries)?);
    } else {
        println!("{}", table::entries_table(&entries));
    }
    Ok(())
}
