//! YouTube player cipher: extract and solve the `sig`/`nsig` JavaScript
//! transforms that protect stream URLs.
//!
//! The extraction regexes port the patterns yt-dlp's `youtube.py` uses: the
//! signature ("sig") transform is a function that splits its argument into a
//! character array, mutates it via a shared helper object, then re-joins it; the
//! "n" ("nsig") transform is a standalone function referenced through a
//! single-element dispatch array. We re-emit each as self-contained JavaScript
//! and hand it to [`crate::jsi::JsFunction`] for execution.

use crate::Error;
use regex::Regex;
use std::sync::OnceLock;

/// Solved cipher functions for one player version.
#[derive(Debug)]
pub(crate) struct SolvedPlayer {
    sig: crate::jsi::JsFunction,
    nsig: Option<crate::jsi::JsFunction>,
    /// The `signatureTimestamp` scraped from the player JS, threaded into the
    /// player request so ciphered (TV-embedded) formats are returned valid.
    sts: Option<u64>,
}

/// Stateless extractor of cipher functions from player JavaScript.
pub(crate) struct PlayerSolver;

impl PlayerSolver {
    /// Extract the `sig` and (optional) `nsig` function sources from player JS and
    /// compile them.
    ///
    /// Returns [`Error::Cipher`] if the signature function cannot be located or
    /// fails to compile. A missing `nsig` is tolerated (older players, or when the
    /// pattern shifts) — only stream URLs that actually carry an `n` parameter will
    /// then fail.
    pub fn from_js(player_js: &str) -> crate::Result<SolvedPlayer> {
        let sig_src = extract_sig_source(player_js)
            .ok_or_else(|| Error::Cipher("could not locate signature function".into()))?;
        let sig = crate::jsi::JsFunction::compile(&sig_src, "sig")?;

        let nsig = match extract_nsig_source(player_js) {
            Some(src) => Some(crate::jsi::JsFunction::compile(&src, "nsig")?),
            None => None,
        };

        let sts = extract_signature_timestamp(player_js);

        Ok(SolvedPlayer { sig, nsig, sts })
    }
}

impl SolvedPlayer {
    /// Solve a `signature`/`s` value.
    ///
    /// The CPU-bound JS evaluation runs on a blocking thread under a timeout (see
    /// [`crate::jsi::JsFunction::call_str_async`]) so it never blocks the async
    /// executor and cannot hang indefinitely on hostile player JS.
    pub async fn solve_sig(&self, s: &str) -> crate::Result<String> {
        self.sig.call_str_async(s).await
    }

    /// Solve an `n` (throttling) parameter value.
    ///
    /// Returns [`Error::Cipher`] if the player carried no recognizable `nsig`
    /// function. Runs off the async executor under a timeout, like [`solve_sig`].
    pub async fn solve_n(&self, n: &str) -> crate::Result<String> {
        match &self.nsig {
            Some(f) => {
                let out = f.call_str_async(n).await?;
                // Real nsig functions reference surrounding globals and include
                // `typeof X==="undefined"` early-return guards; executed
                // standalone (without those globals) they can silently return
                // their input unchanged. A passthrough is therefore a strong
                // signal the extracted function did not actually transform `n`,
                // which yields throttled/expiring stream URLs downstream.
                if out == n {
                    tracing::warn!(
                        n = %n,
                        "nsig solve returned its input unchanged; the n parameter \
                         may be un-transformed (player nsig extraction degraded)"
                    );
                }
                Ok(out)
            }
            None => Err(Error::Cipher("no nsig function in player".into())),
        }
    }

    /// The `signatureTimestamp` scraped from the player JS, if present.
    pub fn signature_timestamp(&self) -> Option<u64> {
        self.sts
    }
}

/// Scrape the `signatureTimestamp` (sts) value from the player JS.
///
/// Real players embed `signatureTimestamp:NNNNN` (or `sts:NNNNN`); the value is
/// threaded into the player request so ciphered formats are returned valid.
fn extract_signature_timestamp(js: &str) -> Option<u64> {
    static STS_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let re = STS_RE
        .get_or_init(|| Regex::new(r"(?:signatureTimestamp|sts)\s*:\s*([0-9]{4,})").ok())
        .as_ref()?;
    re.captures(js)?.get(1)?.as_str().parse().ok()
}

/// Extract the signature transform and re-emit it as standalone JS.
///
/// Mirrors yt-dlp's strategy: first anchor on the *call site* that assigns the
/// deciphered signature (e.g. `...set("signature", FN(decodeURIComponent(c)))`
/// or `c=FN(decodeURIComponent(c))`) to learn the signature function's name,
/// then resolve that named function's body. If no call site is found, fall back
/// to scanning for a `function(a){a=a.split("")...return a.join("")}` body shape.
///
/// In all cases it gathers every helper object the body dereferences
/// (`OBJ.method(...)`), inlines those `var OBJ={...};` declarations, and wraps the
/// body in `function sig(a){...}`.
fn extract_sig_source(js: &str) -> Option<String> {
    // Helper-object reference matcher (`OBJ.method(`), built once.
    static HELPER_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let helper_re = HELPER_RE
        .get_or_init(|| Regex::new(r"([\w$]+)\.[\w$]+\(").ok())
        .as_ref()?;

    // 1) Preferred: anchor on the call site to learn the sig function name, then
    //    resolve its `function NAME(arg){...}` body.
    if let Some((arg, inner)) = find_named_sig_function(js) {
        return Some(emit_sig(js, &arg, &inner, helper_re));
    }

    // 2) Fallback: scan for an anonymous function whose body has the split/join
    //    shape on its own argument.
    static HEADER_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let header_re = HEADER_RE
        .get_or_init(|| Regex::new(r"function\s*\(\s*([\w$]+)\s*\)\s*\{").ok())
        .as_ref()?;

    for caps in header_re.captures_iter(js) {
        let m = caps.get(0)?;
        let arg = caps.get(1)?.as_str();
        let brace_start = m.end() - 1; // position of the opening `{`
        let Some(body) = balanced_braces(&js[brace_start..]) else {
            continue;
        };
        // body still includes the outer braces; strip them.
        let inner = &body[1..body.len() - 1];

        if !body_has_split_join_shape(inner, arg) {
            continue;
        }

        return Some(emit_sig(js, arg, inner, helper_re));
    }

    None
}

/// Whether a function body splits and re-joins `arg` (the signature shape),
/// tolerating leading statements and arbitrary whitespace around the
/// `arg=arg.split("")` / `return arg.join("")` calls.
fn body_has_split_join_shape(inner: &str, arg: &str) -> bool {
    let normalized: String = inner.chars().filter(|c| !c.is_whitespace()).collect();
    let split = format!(r#"{arg}={arg}.split("")"#);
    let join = format!(r#"return{arg}.join("")"#);
    normalized.contains(&split) && normalized.contains(&join)
}

/// Locate the signature function by name via its call site, returning
/// `(arg, body_without_braces)`.
///
/// yt-dlp anchors signature extraction on the call site that assigns the decoded
/// signature, e.g. `a.set("signature", FN(decodeURIComponent(c)))` or
/// `c=FN(decodeURIComponent(c))`. We extract the named function `FN` and balance
/// its body.
fn find_named_sig_function(js: &str) -> Option<(String, String)> {
    static CALLSITE_RE: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = CALLSITE_RE.get_or_init(|| {
        [
            // ...set("signature"|"sig", FN(...   /  ...set("signature", encodeURIComponent(FN(...
            r#"\.set\(\s*"(?:signature|sig)"\s*,\s*(?:encodeURIComponent\s*\(\s*)?([A-Za-z0-9$]+)\("#,
            // c&&(c=FN(decodeURIComponent(c)))   /  c=FN(decodeURIComponent(...
            r#"[A-Za-z0-9$]\s*=\s*([A-Za-z0-9$]+)\(\s*decodeURIComponent\("#,
            // c&&d.set(...,encodeURIComponent(FN(...
            r#"&&\s*[A-Za-z0-9$]+\.set\([^,]+,\s*(?:encodeURIComponent\s*\(\s*)?([A-Za-z0-9$]+)\("#,
        ]
        .into_iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
    });

    for re in patterns {
        for caps in re.captures_iter(js) {
            let Some(name) = caps.get(1).map(|m| m.as_str()) else {
                continue;
            };
            // Reject known non-sig helpers grabbed by a loose call site.
            if name == "decodeURIComponent" || name == "encodeURIComponent" {
                continue;
            }
            if let Some((arg, inner)) = resolve_function_body(js, name) {
                // Validate the shape so we don't wire up an unrelated function.
                if body_has_split_join_shape(&inner, &arg) {
                    return Some((arg, inner));
                }
            }
        }
    }
    None
}

/// Resolve a function named `name` (declared as `function NAME(arg){...}`,
/// `var NAME=function(arg){...}`, or `NAME=function(arg){...}`) into
/// `(arg, body_without_braces)`.
fn resolve_function_body(js: &str, name: &str) -> Option<(String, String)> {
    let decl_pos = find_function_decl(js, name)?;
    let arg_start = js[decl_pos..].find('(')? + decl_pos;
    let arg_end = js[arg_start..].find(')')? + arg_start;
    let arg = js[arg_start + 1..arg_end].trim().to_string();
    let brace_start = js[arg_end..].find('{')? + arg_end;
    let body = balanced_braces(&js[brace_start..])?;
    let inner = body[1..body.len() - 1].to_string();
    Some((arg, inner))
}

/// Emit a self-contained `function sig(arg){...}` plus the helper object
/// declarations the body references.
fn emit_sig(js: &str, arg: &str, inner: &str, helper_re: &Regex) -> String {
    let mut emitted = String::new();
    let mut seen: Vec<&str> = Vec::new();
    for c in helper_re.captures_iter(inner) {
        let obj = c.get(1).map(|g| g.as_str()).unwrap_or_default();
        if obj == arg || obj.is_empty() || seen.contains(&obj) {
            continue;
        }
        seen.push(obj);
        if let Some(decl) = extract_var_object(js, obj) {
            emitted.push_str(&decl);
            emitted.push('\n');
        }
    }
    format!("{emitted}function sig({arg}){{{inner}}}")
}

/// Extract `var NAME={...};` (or `NAME={...};`) with brace balancing.
fn extract_var_object(js: &str, name: &str) -> Option<String> {
    let needle_pos = find_object_decl(js, name)?;
    let brace_start = js[needle_pos..].find('{')? + needle_pos;
    let body = balanced_braces(&js[brace_start..])?;
    Some(format!("var {name}={body};"))
}

/// Locate the start of a `var NAME = {` / `NAME = {` declaration.
fn find_object_decl(js: &str, name: &str) -> Option<usize> {
    for pat in [
        format!("var {name}="),
        format!("var {name} ="),
        format!("{name}="),
    ] {
        let mut from = 0;
        while let Some(rel) = js[from..].find(&pat) {
            let pos = from + rel;
            // Ensure preceded by a non-identifier boundary so we match whole names.
            let ok_before = pos == 0
                || !js[..pos]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$');
            // Ensure the value is an object literal.
            let after = &js[pos + pat.len()..];
            if ok_before && after.trim_start().starts_with('{') {
                return Some(pos);
            }
            from = pos + pat.len();
        }
    }
    None
}

/// Extract the `nsig` transform and re-emit it as standalone JS.
///
/// Mirrors yt-dlp's call-site anchoring: the `n` parameter is transformed via a
/// single-element dispatch array indexed at the `n` call site, e.g.
/// `a.set("n", TBL[0](c))`. We learn the dispatch array name `TBL` from that call
/// site, resolve `var TBL=[FN];` to the function name `FN`, then extract `FN`'s
/// body. This avoids the brittle "first single-element array in the file"
/// heuristic, which a real minified base.js (full of unrelated single-element
/// arrays) would defeat.
///
/// Falls back to scanning every single-element array (not just the first) and
/// accepting the first whose element resolves to a function with a body.
fn extract_nsig_source(js: &str) -> Option<String> {
    let fn_name = find_nsig_function_name(js)?;
    emit_nsig(js, &fn_name)
}

/// Resolve the nsig function name, preferring the dispatch array referenced at
/// the `n` call site.
fn find_nsig_function_name(js: &str) -> Option<String> {
    static TBL_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let tbl_re = TBL_RE
        .get_or_init(|| Regex::new(r"var\s+([\w$]+)\s*=\s*\[\s*([\w$]+)\s*\]").ok())
        .as_ref()?;

    // 1) Preferred: find the array name used at the `n` call site (`TBL[idx](`),
    //    then map that array to its element.
    if let Some(arr_name) = find_nsig_dispatch_array(js) {
        for caps in tbl_re.captures_iter(js) {
            if caps.get(1).map(|m| m.as_str()) == Some(arr_name.as_str()) {
                if let Some(fname) = caps.get(2).map(|m| m.as_str().to_string()) {
                    if find_function_decl(js, &fname).is_some() {
                        return Some(fname);
                    }
                }
            }
        }
    }

    // 2) Fallback: scan ALL single-element arrays and accept the first whose
    //    element resolves to an actual function declaration.
    for caps in tbl_re.captures_iter(js) {
        if let Some(fname) = caps.get(2).map(|m| m.as_str().to_string()) {
            if find_function_decl(js, &fname).is_some() {
                return Some(fname);
            }
        }
    }
    None
}

/// Find the dispatch-array identifier indexed at the `n` parameter call site,
/// e.g. the `nfd` in `a.set("n", nfd[0](c))` or `b=nfd[0](b)`.
fn find_nsig_dispatch_array(js: &str) -> Option<String> {
    static N_CALLSITE_RE: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = N_CALLSITE_RE.get_or_init(|| {
        [
            // ...set("n", TBL[idx](
            r#"\.set\(\s*"n"\s*,\s*([A-Za-z0-9$]+)\[\d+\]\("#,
            // x=TBL[idx](y) near an n-get, generic indexed dispatch call.
            r#"=\s*([A-Za-z0-9$]+)\[\d+\]\([A-Za-z0-9$]+\)"#,
        ]
        .into_iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
    });
    for re in patterns {
        if let Some(caps) = re.captures(js) {
            if let Some(name) = caps.get(1).map(|m| m.as_str().to_string()) {
                return Some(name);
            }
        }
    }
    None
}

/// Emit a self-contained `function nsig(arg){...}` from the named function.
fn emit_nsig(js: &str, fn_name: &str) -> Option<String> {
    let decl_pos = find_function_decl(js, fn_name)?;
    let arg_start = js[decl_pos..].find('(')? + decl_pos;
    let arg_end = js[arg_start..].find(')')? + arg_start;
    let arg = js[arg_start + 1..arg_end].trim();
    let brace_start = js[arg_end..].find('{')? + arg_end;
    let body = balanced_braces(&js[brace_start..])?;

    Some(format!("function nsig({arg}){body}"))
}

/// Locate the start of a `var NAME = function` / `NAME = function` declaration.
fn find_function_decl(js: &str, name: &str) -> Option<usize> {
    for pat in [
        format!("var {name}=function"),
        format!("var {name} =function"),
        format!("var {name}= function"),
        format!("var {name} = function"),
        format!("{name}=function"),
        format!("{name} = function"),
        format!("function {name}"),
    ] {
        if let Some(pos) = js.find(&pat) {
            return Some(pos);
        }
    }
    None
}

/// Given a slice starting at an opening `{`, return the balanced `{...}` block
/// (braces included), ignoring braces inside string/char literals.
fn balanced_braces(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str: Option<u8> = None;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate() {
        if let Some(q) = in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == q {
                in_str = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' | b'`' => in_str = Some(b),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Fetch the player JavaScript for the current YouTube web player, returning a
/// `(version_key, js_text)` pair.
///
/// Discovers the player version from `{base}/iframe_api`, then downloads
/// `{base}/s/player/{version}/player_ias.vflset/en_US/base.js`. The version key is
/// suitable for caching solved players across videos.
pub(crate) async fn fetch_player_js(
    http: &dyn crate::transport::HttpClient,
    base: &str,
) -> crate::Result<(String, String)> {
    use crate::transport::HttpRequest;
    let base = base.trim_end_matches('/');

    let iframe_resp = http
        .execute(HttpRequest::get(
            "fetch_iframe_api",
            format!("{base}/iframe_api"),
        ))
        .await?;
    if !iframe_resp.is_success() {
        return Err(Error::Network {
            stage: "fetch_iframe_api",
            message: format!("status {}", iframe_resp.status),
        });
    }
    let iframe_bytes = super::innertube::read_bounded(iframe_resp, "fetch_iframe_api")?;
    let iframe = String::from_utf8_lossy(&iframe_bytes).into_owned();

    static VER_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let ver_re = VER_RE
        .get_or_init(|| Regex::new(r"player\\?/([0-9a-fA-F]{8})\\?/").ok())
        .as_ref()
        .ok_or_else(|| Error::Cipher("player version regex failed to compile".into()))?;
    let version = ver_re
        .captures(&iframe)
        .and_then(|c| c.get(1))
        .ok_or_else(|| Error::Extraction {
            stage: "player_version",
            message: "could not find player version in iframe_api".into(),
        })?
        .as_str()
        .to_string();

    let player_url = format!("{base}/s/player/{version}/player_ias.vflset/en_US/base.js");
    let js_resp = http
        .execute(HttpRequest::get("fetch_player_js", player_url))
        .await?;
    if !js_resp.is_success() {
        return Err(Error::Network {
            stage: "fetch_player_js",
            message: format!("status {}", js_resp.status),
        });
    }
    let js_bytes = super::innertube::read_bounded(js_resp, "fetch_player_js")?;
    let js = String::from_utf8_lossy(&js_bytes).into_owned();

    Ok((version, js))
}

/// Apply a solved player to a [`RawFormat`]'s `signatureCipher` or direct URL,
/// producing the final, playable URL.
///
/// For a `signatureCipher` blob, parses `s`/`sp`/`url`, solves `s`, and appends
/// `{sp}={solved}` to the URL. For a direct URL (or after appending the signature),
/// any `n` query parameter is replaced with its solved value when an `nsig`
/// function is available.
pub(crate) async fn decipher_url(
    raw: &super::innertube::RawFormat,
    player: &SolvedPlayer,
) -> crate::Result<String> {
    let base = if let Some(cipher) = &raw.signature_cipher {
        let mut s = None;
        let mut sp = None;
        let mut url = None;
        for (k, v) in url::form_urlencoded::parse(cipher.as_bytes()) {
            match k.as_ref() {
                "s" => s = Some(v.into_owned()),
                "sp" => sp = Some(v.into_owned()),
                "url" => url = Some(v.into_owned()),
                _ => {}
            }
        }
        let url = url.ok_or_else(|| Error::Cipher("signatureCipher missing url field".into()))?;
        let s = s.ok_or_else(|| Error::Cipher("signatureCipher missing s field".into()))?;
        let sp = sp.unwrap_or_else(|| "signature".to_string());
        let solved = player.solve_sig(&s).await?;
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}{sp}={solved}")
    } else if let Some(url) = &raw.url {
        url.clone()
    } else {
        return Err(Error::Cipher(
            "format has neither url nor signatureCipher".into(),
        ));
    };

    transform_n_param(&base, player).await
}

/// Replace an `n=` query parameter with its solved value, if present and solvable.
async fn transform_n_param(url: &str, player: &SolvedPlayer) -> crate::Result<String> {
    let mut parsed = match url::Url::parse(url) {
        Ok(p) => p,
        // Not absolute / unparseable: leave untouched rather than fail the format.
        Err(_) => return Ok(url.to_string()),
    };
    let n = parsed
        .query_pairs()
        .find(|(k, _)| k == "n")
        .map(|(_, v)| v.into_owned());
    let Some(n) = n else {
        return Ok(url.to_string());
    };
    let solved = player.solve_n(&n).await?;
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            if k == "n" {
                (k.into_owned(), solved.clone())
            } else {
                (k.into_owned(), v.into_owned())
            }
        })
        .collect();
    {
        let mut qp = parsed.query_pairs_mut();
        qp.clear();
        for (k, v) in &pairs {
            qp.append_pair(k, v);
        }
    }
    Ok(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNTHETIC: &str = include_str!("../../../tests/fixtures/player/synthetic_player.js");

    #[tokio::test]
    async fn extracts_and_solves_sig_from_synthetic_player() {
        let player = PlayerSolver::from_js(SYNTHETIC).unwrap();
        assert_eq!(player.solve_sig("0123456789").await.unwrap(), "67543210");
    }

    #[tokio::test]
    async fn extracts_and_solves_nsig() {
        let player = PlayerSolver::from_js(SYNTHETIC).unwrap();
        assert_eq!(player.solve_n("abcdef").await.unwrap(), "kjihgf");
    }

    /// Finding 7: a guard-style nsig function (one that returns its input
    /// unchanged when a surrounding global is undefined, as real player nsig
    /// functions do when executed standalone) must still succeed but emit a
    /// `tracing::warn`. We install a minimal subscriber that records whether any
    /// WARN-level event fired during the solve.
    #[tokio::test]
    async fn nsig_passthrough_emits_warning() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use tracing::span;

        // A minimal subscriber that flips a flag when a WARN event is recorded.
        struct WarnCatcher(Arc<AtomicBool>);
        impl tracing::Subscriber for WarnCatcher {
            fn enabled(&self, meta: &tracing::Metadata<'_>) -> bool {
                *meta.level() <= tracing::Level::WARN
            }
            fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
                span::Id::from_u64(1)
            }
            fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
            fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
            fn event(&self, event: &tracing::Event<'_>) {
                if *event.metadata().level() == tracing::Level::WARN {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            fn enter(&self, _: &span::Id) {}
            fn exit(&self, _: &span::Id) {}
        }

        // A guard-style nsig: `if (typeof glob === "undefined") return a;` — when
        // run standalone (no `glob`), it returns its argument unchanged.
        let guarded = crate::jsi::JsFunction::compile(
            r#"function nsig(a){if(typeof glob==="undefined")return a;return a+"!"}"#,
            "nsig",
        )
        .unwrap();
        let player = SolvedPlayer {
            sig: crate::jsi::JsFunction::compile(
                r#"function sig(a){return a.split("").join("")}"#,
                "sig",
            )
            .unwrap(),
            nsig: Some(guarded),
            sts: None,
        };

        let warned = Arc::new(AtomicBool::new(false));
        let subscriber = WarnCatcher(warned.clone());
        let out = tracing::subscriber::with_default(subscriber, || {
            futures::executor::block_on(player.solve_n("untouched"))
        })
        .unwrap();

        assert_eq!(out, "untouched", "guarded nsig returns input unchanged");
        assert!(
            warned.load(Ordering::SeqCst),
            "a passthrough nsig solve must emit a tracing::warn"
        );
    }

    #[test]
    fn nsig_extraction_ignores_decoy_arrays_via_call_site() {
        // Regression for the "first single-element array wins" defect: decoy
        // single-element arrays (`var aa=[za]`, `var ab=[zb]`) appear before the
        // real nsig table `var nfd=[bna]`. The extractor must anchor on the `n`
        // call site (`nfd[0](c)`) and resolve `bna`, not the decoys.
        let name = find_nsig_function_name(SYNTHETIC).expect("nsig name resolved");
        assert_eq!(name, "bna");
        // The dispatch array is found from the call site, not the first array.
        assert_eq!(find_nsig_dispatch_array(SYNTHETIC).as_deref(), Some("nfd"));
    }

    #[test]
    fn sig_extraction_handles_leading_statement_via_call_site() {
        // Regression for the brittle "body must start with a=a.split" heuristic:
        // the real sig fn `ada` has a leading `var z=0;` before the split. The
        // extractor anchors on the call site `set("signature", ada(...))` and
        // resolves it anyway.
        let src = extract_sig_source(SYNTHETIC).expect("sig source extracted");
        // Must wire up the helper object `Ix` it references.
        assert!(src.contains("var Ix="), "helper object inlined: {src}");
        assert!(src.contains("function sig("), "wrapped as sig: {src}");
    }

    #[test]
    fn unparseable_player_yields_cipher_error() {
        let err = PlayerSolver::from_js("var x = 1;").unwrap_err();
        assert!(matches!(err, crate::Error::Cipher(_)));
    }

    #[tokio::test]
    async fn parses_signature_cipher_param() {
        // A solved player that reverses the signature value.
        let reverse = crate::jsi::JsFunction::compile(
            r#"function sig(a){return a.split("").reverse().join("")}"#,
            "sig",
        )
        .unwrap();
        let player = SolvedPlayer {
            sig: reverse,
            nsig: None,
            sts: None,
        };
        let raw = raw_with_cipher("s=ABC&sp=sig&url=https%3A%2F%2Fr.test%2Fv");
        let url = decipher_url(&raw, &player).await.unwrap();
        assert_eq!(url, "https://r.test/v?sig=CBA");
    }

    #[tokio::test]
    async fn signature_cipher_default_sp_is_signature() {
        let reverse = crate::jsi::JsFunction::compile(
            r#"function sig(a){return a.split("").reverse().join("")}"#,
            "sig",
        )
        .unwrap();
        let player = SolvedPlayer {
            sig: reverse,
            nsig: None,
            sts: None,
        };
        let raw = raw_with_cipher("s=ABC&url=https%3A%2F%2Fr.test%2Fv");
        let url = decipher_url(&raw, &player).await.unwrap();
        assert_eq!(url, "https://r.test/v?signature=CBA");
    }

    #[tokio::test]
    async fn direct_url_with_n_param_is_transformed() {
        let player = PlayerSolver::from_js(SYNTHETIC).unwrap();
        let mut raw = raw_with_cipher("");
        raw.signature_cipher = None;
        raw.url = Some("https://r.test/v?id=1&n=abcdef".into());
        let url = decipher_url(&raw, &player).await.unwrap();
        assert_eq!(url, "https://r.test/v?id=1&n=kjihgf");
    }

    #[tokio::test]
    async fn direct_url_without_cipher_passes_through() {
        let player = PlayerSolver::from_js(SYNTHETIC).unwrap();
        let mut raw = raw_with_cipher("");
        raw.signature_cipher = None;
        raw.url = Some("https://r.test/v?id=1".into());
        let url = decipher_url(&raw, &player).await.unwrap();
        assert_eq!(url, "https://r.test/v?id=1");
    }

    #[tokio::test]
    async fn fetch_player_js_discovers_version_and_downloads_base_js() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let iframe_body = r#"if(!window.YT){...}var u="\/s\/player\/abcd1234\/www-widgetapi.js";"#;
        Mock::given(method("GET"))
            .and(path("/iframe_api"))
            .respond_with(ResponseTemplate::new(200).set_body_string(iframe_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/s/player/abcd1234/player_ias.vflset/en_US/base.js"))
            .respond_with(ResponseTemplate::new(200).set_body_string("var sig=1;"))
            .mount(&server)
            .await;

        let client = crate::transport::ReqwestClient::new(reqwest::Client::new());
        let (version, js) = fetch_player_js(&client, &server.uri()).await.unwrap();
        assert_eq!(version, "abcd1234");
        assert_eq!(js, "var sig=1;");
    }

    #[tokio::test]
    async fn fetch_player_js_without_version_is_extraction_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/iframe_api"))
            .respond_with(ResponseTemplate::new(200).set_body_string("no version here"))
            .mount(&server)
            .await;

        let client = crate::transport::ReqwestClient::new(reqwest::Client::new());
        let err = fetch_player_js(&client, &server.uri()).await.unwrap_err();
        assert!(matches!(err, crate::Error::Extraction { .. }));
    }

    fn raw_with_cipher(cipher: &str) -> super::super::innertube::RawFormat {
        super::super::innertube::RawFormat {
            itag: 251,
            url: None,
            signature_cipher: Some(cipher.to_string()),
            mime_type: "audio/webm; codecs=\"opus\"".into(),
            width: None,
            height: None,
            fps: None,
            bitrate: Some(160_000),
            content_length: None,
            audio_sample_rate: Some("48000".into()),
            audio_channels: Some(2),
        }
    }
}
