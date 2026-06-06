//! Lazy continuation-based pagination for YouTube collections.

use futures::stream;
use serde_json::Value;
use std::sync::Arc;

use super::innertube::{BrowseRequest, InnerTube};
use crate::types::{Entry, EntryStream, Thumbnail};

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

/// Maximum number of continuation pages fetched for a single collection.
///
/// A hard cap against an adversarial or buggy InnerTube response that keeps
/// returning continuation tokens forever (including a token cycle). 10k pages of
/// ~100 entries comfortably exceeds any real playlist/channel/search while
/// bounding the work an iterate-to-completion consumer can be made to do.
const MAX_PAGES: u32 = 10_000;

/// Maximum number of *consecutive* continuation pages that may yield zero entries
/// before pagination gives up.
///
/// `MAX_PAGES` and seen-token cycle detection do not stop a server that hands
/// back a FRESH continuation token on every page while never producing an entry:
/// such a stream makes no progress yet defeats cycle detection and could spin up
/// to `MAX_PAGES` fetches. A real collection interleaves entries with its
/// continuations, so a short run of entry-less pages is a reliable "no progress"
/// signal to terminate on.
const MAX_CONSECUTIVE_EMPTY_PAGES: u32 = 3;

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
    /// Number of continuation pages fetched so far (bounds infinite streams).
    pages_fetched: u32,
    /// Continuation tokens already seen, to break token cycles.
    seen_tokens: std::collections::HashSet<String>,
    /// Consecutive continuation fetches that produced no entries (no-progress guard).
    consecutive_empty: u32,
}

/// Turn an initial browse/search response into a lazy entry stream.
///
/// Drives [`stream::unfold`] over an `Option<continuation_token>`: each step
/// drains buffered entries, and only when the buffer empties does it fetch the
/// next page. The stream ends once no continuation token remains.
pub(crate) fn entry_stream(it: Arc<InnerTube>, first_page: Value, kind: PageKind) -> EntryStream {
    let state = PageState {
        it,
        kind,
        next: None,
        buffer: std::collections::VecDeque::new(),
        first_page: Some(first_page),
        pages_fetched: 0,
        seen_tokens: std::collections::HashSet::new(),
        consecutive_empty: 0,
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

            // Bound total pages and break continuation-token cycles, defending
            // against adversarial/buggy responses that paginate forever.
            if state.pages_fetched >= MAX_PAGES || !state.seen_tokens.insert(token.clone()) {
                return None;
            }
            state.pages_fetched += 1;

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
                    // No-progress guard: a page that yields no entries but keeps
                    // handing back a (fresh) continuation token makes no progress
                    // yet evades both MAX_PAGES (slowly) and cycle detection
                    // (tokens differ). Stop after a short run of empty pages.
                    if entries.is_empty() {
                        state.consecutive_empty += 1;
                        if state.consecutive_empty >= MAX_CONSECUTIVE_EMPTY_PAGES {
                            return None;
                        }
                    } else {
                        state.consecutive_empty = 0;
                    }
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
            Arc::new(crate::transport::ReqwestClient::new(reqwest::Client::new())),
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

    /// Regression for the unbounded-pagination defect: a response that always
    /// returns the SAME continuation token (a token cycle) must terminate via
    /// cycle detection instead of looping forever.
    #[tokio::test]
    async fn pagination_breaks_continuation_token_cycle() {
        let server = MockServer::start().await;

        // Every browse continuation returns one entry AND the same token again.
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/browse"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "onResponseReceivedActions": [{
                    "appendContinuationItemsAction": {
                        "continuationItems": [
                            { "playlistVideoRenderer": { "videoId": "loopvideo00", "title": { "runs": [{ "text": "Loop" }] } } },
                            { "continuationItemRenderer": { "continuationEndpoint": {
                                "continuationCommand": { "token": "SAME_TOKEN" } } } }
                        ]
                    }
                }]
            })))
            .mount(&server)
            .await;

        let it = Arc::new(InnerTube::with_base_url(
            Arc::new(crate::transport::ReqwestClient::new(reqwest::Client::new())),
            server.uri(),
        ));
        // First page yields one entry and the looping token.
        let first = serde_json::json!({
            "contents": { "x": [
                { "playlistVideoRenderer": { "videoId": "firstentry0", "title": { "runs": [{ "text": "First" }] } } },
                { "continuationItemRenderer": { "continuationEndpoint": {
                    "continuationCommand": { "token": "SAME_TOKEN" } } } }
            ]}
        });

        let entries: Vec<Entry> = entry_stream(it, first, PageKind::Playlist)
            .map(|r| r.expect("entry"))
            .collect()
            .await;

        // First page entry + exactly one continuation fetch before the cycle is
        // detected and the stream terminates.
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["firstentry0", "loopvideo00"]);
    }

    /// Finding 1: a server that returns NO entries but a FRESH continuation
    /// token on every page makes no progress yet defeats cycle detection (tokens
    /// always differ). The no-progress guard must terminate the stream after a
    /// short run of empty pages instead of spinning up to MAX_PAGES.
    #[tokio::test]
    async fn pagination_stops_on_no_progress_fresh_tokens() {
        let server = MockServer::start().await;
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let hits_resp = hits.clone();

        // Each continuation page: zero entries, but a brand-new token each time.
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/browse"))
            .respond_with(move |_req: &Request| {
                let n = hits_resp.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "onResponseReceivedActions": [{ "appendContinuationItemsAction": {
                        "continuationItems": [
                            { "continuationItemRenderer": { "continuationEndpoint": {
                                "continuationCommand": { "token": format!("FRESH_{n}") } } } }
                        ]
                    }}]
                }))
            })
            .mount(&server)
            .await;

        let it = Arc::new(InnerTube::with_base_url(
            Arc::new(crate::transport::ReqwestClient::new(reqwest::Client::new())),
            server.uri(),
        ));
        // First page: one entry + a fresh continuation token.
        let first = serde_json::json!({
            "contents": { "x": [
                { "playlistVideoRenderer": { "videoId": "firstentry0", "title": { "runs": [{ "text": "First" }] } } },
                { "continuationItemRenderer": { "continuationEndpoint": {
                    "continuationCommand": { "token": "FRESH_START" } } } }
            ]}
        });

        let entries: Vec<Entry> = entry_stream(it, first, PageKind::Playlist)
            .map(|r| r.expect("entry"))
            .collect()
            .await;

        // Only the first-page entry; the empty-page run terminates the stream.
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["firstentry0"]);
        // Far fewer than MAX_PAGES: bounded by MAX_CONSECUTIVE_EMPTY_PAGES.
        assert!(
            hits.load(std::sync::atomic::Ordering::SeqCst) <= MAX_CONSECUTIVE_EMPTY_PAGES,
            "no-progress guard must stop after a few empty pages"
        );
    }

    /// Regression for finding 19: search continuation must re-issue via the
    /// search endpoint (query re-sent on page 1, continuation-only on page 2),
    /// and entries from both pages must be yielded.
    #[tokio::test]
    async fn search_stream_paginates_via_search_endpoint() {
        let server = MockServer::start().await;

        // Page 1 (no continuation): videoRenderer + a continuation token.
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/search"))
            .and(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                body.get("continuation").is_none() && body["query"] == "rust"
            })
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "contents": { "twoColumnSearchResultsRenderer": { "primaryContents": {
                    "sectionListRenderer": { "contents": [
                        { "itemSectionRenderer": { "contents": [
                            { "videoRenderer": { "videoId": "searchres01", "title": { "runs": [{ "text": "S1" }] } } }
                        ]}},
                        { "continuationItemRenderer": { "continuationEndpoint": {
                            "continuationCommand": { "token": "SEARCH_TOK_2" } } } }
                    ]}
                }}}
            })))
            .mount(&server)
            .await;

        // Page 2 (continuation, query re-sent): videoRenderer, no further token.
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/search"))
            .and(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                body.get("continuation").and_then(Value::as_str) == Some("SEARCH_TOK_2")
            })
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "onResponseReceivedCommands": [{ "appendContinuationItemsAction": {
                    "continuationItems": [
                        { "itemSectionRenderer": { "contents": [
                            { "videoRenderer": { "videoId": "searchres02", "title": { "runs": [{ "text": "S2" }] } } }
                        ]}}
                    ]
                }}]
            })))
            .mount(&server)
            .await;

        let it = Arc::new(InnerTube::with_base_url(
            Arc::new(crate::transport::ReqwestClient::new(reqwest::Client::new())),
            server.uri(),
        ));
        // First page is fetched up front, like the real extractor does.
        let first = it.search("rust", None).await.unwrap();

        let entries: Vec<Entry> = entry_stream(it, first, PageKind::Search("rust".to_string()))
            .map(|r| r.expect("entry"))
            .collect()
            .await;
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["searchres01", "searchres02"]);
    }

    /// Regression for finding 19: the channel `richItemRenderer` -> `videoRenderer`
    /// unwrap path must be exercised and produce entries.
    #[tokio::test]
    async fn channel_stream_unwraps_rich_item_renderer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/browse"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "onResponseReceivedActions": [{ "appendContinuationItemsAction": {
                    "continuationItems": [
                        { "richItemRenderer": { "content": {
                            "videoRenderer": { "videoId": "chanvideo02", "title": { "runs": [{ "text": "C2" }] } }
                        }}}
                    ]
                }}]
            })))
            .mount(&server)
            .await;

        let it = Arc::new(InnerTube::with_base_url(
            Arc::new(crate::transport::ReqwestClient::new(reqwest::Client::new())),
            server.uri(),
        ));
        let first = serde_json::json!({
            "contents": { "x": [
                { "richItemRenderer": { "content": {
                    "videoRenderer": { "videoId": "chanvideo01", "title": { "runs": [{ "text": "C1" }] } }
                }}},
                { "continuationItemRenderer": { "continuationEndpoint": {
                    "continuationCommand": { "token": "CHAN_TOK_2" } } } }
            ]}
        });

        let entries: Vec<Entry> = entry_stream(it, first, PageKind::Channel)
            .map(|r| r.expect("entry"))
            .collect()
            .await;
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["chanvideo01", "chanvideo02"]);
    }
}
