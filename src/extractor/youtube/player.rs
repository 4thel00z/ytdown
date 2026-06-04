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

        Ok(SolvedPlayer { sig, nsig })
    }
}

impl SolvedPlayer {
    /// Solve a `signature`/`s` value.
    pub fn solve_sig(&self, s: &str) -> crate::Result<String> {
        self.sig.call_str(s)
    }

    /// Solve an `n` (throttling) parameter value.
    ///
    /// Returns [`Error::Cipher`] if the player carried no recognizable `nsig`
    /// function.
    pub fn solve_n(&self, n: &str) -> crate::Result<String> {
        match &self.nsig {
            Some(f) => f.call_str(n),
            None => Err(Error::Cipher("no nsig function in player".into())),
        }
    }
}

/// Extract the signature transform and re-emit it as standalone JS.
///
/// Locates the function body of the form `a=a.split("");...;return a.join("")`,
/// gathers every helper object it dereferences (`OBJ.method(...)`), inlines those
/// `var OBJ={...};` declarations, and wraps the body in `function sig(a){...}`.
fn extract_sig_source(js: &str) -> Option<String> {
    // The `regex` crate has no backreferences, so we can't assert
    // "split into the same var that's joined" inside the pattern. Instead we
    // locate every function header `... function (arg) {`, balance-match its body,
    // and verify the split/join shape on `arg` in code.
    static HEADER_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let header_re = HEADER_RE
        .get_or_init(|| Regex::new(r"function\s*\(\s*([\w$]+)\s*\)\s*\{").ok())
        .as_ref()?;

    // Helper-object reference matcher (`OBJ.method(`), built once.
    static HELPER_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let helper_re = HELPER_RE
        .get_or_init(|| Regex::new(r"([\w$]+)\.[\w$]+\(").ok())
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
        let trimmed = inner.trim();

        let split_prefix = format!(r#"{arg}={arg}.split("")"#);
        let split_prefix_spaced = format!(r#"{arg} = {arg}.split("")"#);
        let join_suffix = format!(r#"return {arg}.join("")"#);

        let starts_ok =
            trimmed.starts_with(&split_prefix) || trimmed.starts_with(&split_prefix_spaced);
        let contains_join = trimmed.contains(&join_suffix);
        if !(starts_ok && contains_join) {
            continue;
        }

        // Found the signature function body. Collect helper objects it references.
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

        return Some(format!("{emitted}function sig({arg}){{{inner}}}"));
    }

    None
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
/// Finds the single-element dispatch array `var TBL=[FN];`, then extracts the full
/// source of `var FN=function(a){...}` and renames the wrapper to `nsig`.
fn extract_nsig_source(js: &str) -> Option<String> {
    static TBL_RE: OnceLock<Option<Regex>> = OnceLock::new();
    let tbl_re = TBL_RE
        .get_or_init(|| Regex::new(r"var\s+[\w$]+\s*=\s*\[\s*([\w$]+)\s*\]").ok())
        .as_ref()?;
    let fn_name = tbl_re.captures(js)?.get(1)?.as_str();

    // FN = function(arg){ ... }  (capture the arg and body via brace balancing)
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
    http: &reqwest::Client,
    base: &str,
) -> crate::Result<(String, String)> {
    let base = base.trim_end_matches('/');
    let iframe_url = format!("{base}/iframe_api");
    let iframe = http
        .get(&iframe_url)
        .send()
        .await
        .map_err(|source| Error::Network {
            stage: "fetch_iframe_api",
            source,
        })?
        .error_for_status()
        .map_err(|source| Error::Network {
            stage: "fetch_iframe_api",
            source,
        })?
        .text()
        .await
        .map_err(|source| Error::Network {
            stage: "fetch_iframe_api",
            source,
        })?;

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
    let js = http
        .get(&player_url)
        .send()
        .await
        .map_err(|source| Error::Network {
            stage: "fetch_player_js",
            source,
        })?
        .error_for_status()
        .map_err(|source| Error::Network {
            stage: "fetch_player_js",
            source,
        })?
        .text()
        .await
        .map_err(|source| Error::Network {
            stage: "fetch_player_js",
            source,
        })?;

    Ok((version, js))
}

/// Apply a solved player to a [`RawFormat`]'s `signatureCipher` or direct URL,
/// producing the final, playable URL.
///
/// For a `signatureCipher` blob, parses `s`/`sp`/`url`, solves `s`, and appends
/// `{sp}={solved}` to the URL. For a direct URL (or after appending the signature),
/// any `n` query parameter is replaced with its solved value when an `nsig`
/// function is available.
pub(crate) fn decipher_url(
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
        let solved = player.solve_sig(&s)?;
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}{sp}={solved}")
    } else if let Some(url) = &raw.url {
        url.clone()
    } else {
        return Err(Error::Cipher(
            "format has neither url nor signatureCipher".into(),
        ));
    };

    transform_n_param(&base, player)
}

/// Replace an `n=` query parameter with its solved value, if present and solvable.
fn transform_n_param(url: &str, player: &SolvedPlayer) -> crate::Result<String> {
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
    let solved = player.solve_n(&n)?;
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

    #[test]
    fn extracts_and_solves_sig_from_synthetic_player() {
        let player = PlayerSolver::from_js(SYNTHETIC).unwrap();
        assert_eq!(player.solve_sig("0123456789").unwrap(), "67543210");
    }

    #[test]
    fn extracts_and_solves_nsig() {
        let player = PlayerSolver::from_js(SYNTHETIC).unwrap();
        assert_eq!(player.solve_n("abcdef").unwrap(), "kjihgf");
    }

    #[test]
    fn unparseable_player_yields_cipher_error() {
        let err = PlayerSolver::from_js("var x = 1;").unwrap_err();
        assert!(matches!(err, crate::Error::Cipher(_)));
    }

    #[test]
    fn parses_signature_cipher_param() {
        // A solved player that reverses the signature value.
        let reverse = crate::jsi::JsFunction::compile(
            r#"function sig(a){return a.split("").reverse().join("")}"#,
            "sig",
        )
        .unwrap();
        let player = SolvedPlayer {
            sig: reverse,
            nsig: None,
        };
        let raw = raw_with_cipher("s=ABC&sp=sig&url=https%3A%2F%2Fr.test%2Fv");
        let url = decipher_url(&raw, &player).unwrap();
        assert_eq!(url, "https://r.test/v?sig=CBA");
    }

    #[test]
    fn signature_cipher_default_sp_is_signature() {
        let reverse = crate::jsi::JsFunction::compile(
            r#"function sig(a){return a.split("").reverse().join("")}"#,
            "sig",
        )
        .unwrap();
        let player = SolvedPlayer {
            sig: reverse,
            nsig: None,
        };
        let raw = raw_with_cipher("s=ABC&url=https%3A%2F%2Fr.test%2Fv");
        let url = decipher_url(&raw, &player).unwrap();
        assert_eq!(url, "https://r.test/v?signature=CBA");
    }

    #[test]
    fn direct_url_with_n_param_is_transformed() {
        let player = PlayerSolver::from_js(SYNTHETIC).unwrap();
        let mut raw = raw_with_cipher("");
        raw.signature_cipher = None;
        raw.url = Some("https://r.test/v?id=1&n=abcdef".into());
        let url = decipher_url(&raw, &player).unwrap();
        assert_eq!(url, "https://r.test/v?id=1&n=kjihgf");
    }

    #[test]
    fn direct_url_without_cipher_passes_through() {
        let player = PlayerSolver::from_js(SYNTHETIC).unwrap();
        let mut raw = raw_with_cipher("");
        raw.signature_cipher = None;
        raw.url = Some("https://r.test/v?id=1".into());
        let url = decipher_url(&raw, &player).unwrap();
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

        let (version, js) = fetch_player_js(&reqwest::Client::new(), &server.uri())
            .await
            .unwrap();
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

        let err = fetch_player_js(&reqwest::Client::new(), &server.uri())
            .await
            .unwrap_err();
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
