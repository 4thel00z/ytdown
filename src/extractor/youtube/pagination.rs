//! Lazy continuation-based pagination for YouTube collections.

use futures::stream::{self, BoxStream};
use serde_json::Value;
use std::sync::Arc;

use super::innertube::{BrowseRequest, InnerTube};
use crate::types::{Entry, Thumbnail};

/// Which kind of collection a page belongs to. Renderer paths differ by kind.
///
/// [`PageKind::Search`] carries the original query because continuation pages
/// for search go through the `search` endpoint, which requires it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PageKind {
    /// A playlist (`playlistVideoRenderer`).
    Playlist,
    /// A channel's uploads (`richItemRenderer` / `videoRenderer`).
    Channel,
    /// Search results (`videoRenderer`) for the given query.
    Search(String),
}

/// Per-step state threaded through the [`stream::unfold`] driving pagination.
struct PageState {
    it: Arc<InnerTube>,
    kind: PageKind,
    /// Pending continuation token; `None` once the collection is exhausted.
    next: Option<String>,
    /// Entries parsed from the current page but not yet yielded.
    buffer: std::collections::VecDeque<Entry>,
    /// Whether the first (already-fetched) page still needs parsing.
    first_page: Option<Value>,
}

/// Turn an initial browse/search response into a lazy entry stream.
///
/// Drives [`stream::unfold`] over an `Option<continuation_token>`: each step
/// drains buffered entries, and only when the buffer empties does it fetch the
/// next page. The stream ends once no continuation token remains.
pub(crate) fn entry_stream(
    it: Arc<InnerTube>,
    first_page: Value,
    kind: PageKind,
) -> BoxStream<'static, crate::Result<Entry>> {
    let state = PageState {
        it,
        kind,
        next: None,
        buffer: std::collections::VecDeque::new(),
        first_page: Some(first_page),
    };

    let stream = stream::unfold(state, |mut state| async move {
        loop {
            // Yield any buffered entry first.
            if let Some(entry) = state.buffer.pop_front() {
                return Some((Ok(entry), state));
            }

            // Parse the initial page lazily on first poll.
            if let Some(page) = state.first_page.take() {
                let (entries, token) = parse_entries(&page, &state.kind);
                state.buffer.extend(entries);
                state.next = token;
                continue;
            }

            // Fetch the next page only when there is a continuation token.
            let token = state.next.take()?;
            let fetched = match &state.kind {
                PageKind::Playlist | PageKind::Channel => {
                    state
                        .it
                        .browse(BrowseRequest {
                            continuation: Some(token),
                            ..BrowseRequest::default()
                        })
                        .await
                }
                PageKind::Search(query) => state.it.search(query, Some(&token)).await,
            };
            match fetched {
                Ok(page) => {
                    let (entries, next) = parse_entries(&page, &state.kind);
                    state.buffer.extend(entries);
                    state.next = next;
                    // Loop: drain the freshly-filled buffer (or stop if empty).
                }
                Err(e) => return Some((Err(e), state)),
            }
        }
    });

    Box::pin(stream)
}

/// Parse the entries and the next continuation token from a single page.
///
/// Renderer layouts vary by collection kind and between the initial page and
/// continuation responses, so this walks the tree recursively: it collects
/// every `playlistVideoRenderer`/`videoRenderer`/`richItemRenderer` it finds
/// and picks up the first `continuationCommand` token anywhere in the tree.
fn parse_entries(page: &Value, _kind: &PageKind) -> (Vec<Entry>, Option<String>) {
    // The recursive scan recognizes every collection kind's renderer, so the
    // `kind` hint is not needed to disambiguate paths in v1; it is retained in
    // the signature for callers and future path-specific optimizations.
    let mut entries = Vec::new();
    let mut token = None;
    collect(page, &mut entries, &mut token);
    (entries, token)
}

/// Recursive walk collecting entries and the continuation token.
fn collect(node: &Value, entries: &mut Vec<Entry>, token: &mut Option<String>) {
    match node {
        Value::Object(map) => {
            for (key, child) in map {
                match key.as_str() {
                    "playlistVideoRenderer" | "videoRenderer" => {
                        if let Some(entry) = parse_renderer(child) {
                            entries.push(entry);
                        }
                    }
                    "richItemRenderer" => {
                        // richItemRenderer wraps a videoRenderer under `content`.
                        if let Some(inner) =
                            child.get("content").and_then(|c| c.get("videoRenderer"))
                        {
                            if let Some(entry) = parse_renderer(inner) {
                                entries.push(entry);
                            }
                        } else {
                            collect(child, entries, token);
                        }
                    }
                    "continuationCommand" => {
                        if token.is_none() {
                            if let Some(t) = child.get("token").and_then(Value::as_str) {
                                *token = Some(t.to_string());
                            }
                        }
                    }
                    _ => collect(child, entries, token),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect(item, entries, token);
            }
        }
        _ => {}
    }
}

/// Build an [`Entry`] from a single `*VideoRenderer` object.
fn parse_renderer(r: &Value) -> Option<Entry> {
    let id = r.get("videoId").and_then(Value::as_str)?.to_string();
    let title = extract_text(r.get("title"));
    let duration = r
        .get("lengthSeconds")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok())
        .map(std::time::Duration::from_secs);
    let thumbnails = r
        .get("thumbnail")
        .and_then(|t| t.get("thumbnails"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(parse_thumbnail).collect())
        .unwrap_or_default();
    Some(Entry {
        url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        title,
        duration,
        thumbnails,
    })
}

/// Parse one thumbnail object.
fn parse_thumbnail(t: &Value) -> Option<Thumbnail> {
    let url = t.get("url").and_then(Value::as_str)?.to_string();
    Some(Thumbnail {
        url,
        width: t.get("width").and_then(Value::as_u64).map(|w| w as u32),
        height: t.get("height").and_then(Value::as_u64).map(|h| h as u32),
    })
}

/// Extract a display string from a YouTube text node (`{"runs":[{"text":..}]}`
/// or `{"simpleText":..}`).
fn extract_text(node: Option<&Value>) -> Option<String> {
    let node = node?;
    if let Some(simple) = node.get("simpleText").and_then(Value::as_str) {
        return Some(simple.to_string());
    }
    let runs = node.get("runs").and_then(Value::as_array)?;
    let mut out = String::new();
    for run in runs {
        if let Some(t) = run.get("text").and_then(Value::as_str) {
            out.push_str(t);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn fixture(name: &str) -> Value {
        let raw = std::fs::read_to_string(format!(
            "{}/tests/fixtures/innertube/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("fixture readable");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    #[tokio::test]
    async fn playlist_stream_paginates_until_no_continuation() {
        let server = MockServer::start().await;

        // The continuation (page 2) browse call returns one entry, no token.
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/browse"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                assert_eq!(
                    body.get("continuation").and_then(Value::as_str),
                    Some("CONT_TOKEN_PAGE2"),
                    "continuation token must be forwarded in the browse body"
                );
                ResponseTemplate::new(200)
                    .set_body_json(super::tests::fixture("browse_playlist_page2.json"))
            })
            .expect(1)
            .mount(&server)
            .await;

        let it = Arc::new(InnerTube::with_base_url(
            reqwest::Client::new(),
            server.uri(),
        ));
        let first = fixture("browse_playlist_page1.json");

        let entries: Vec<Entry> = entry_stream(it, first, PageKind::Playlist)
            .map(|r| r.expect("entry"))
            .collect()
            .await;

        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc"]);
        assert_eq!(
            entries[0].url,
            "https://www.youtube.com/watch?v=aaaaaaaaaaa"
        );
        assert_eq!(entries[0].title.as_deref(), Some("First Video"));
        assert_eq!(
            entries[0].duration,
            Some(std::time::Duration::from_secs(100))
        );
        assert_eq!(
            entries[2].duration,
            Some(std::time::Duration::from_secs(300))
        );
    }
}
