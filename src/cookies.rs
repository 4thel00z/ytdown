//! Netscape `cookies.txt` support and an authenticating transport decorator.
//!
//! YouTube's anti-bot wall ("Sign in to confirm you're not a bot") flags whole
//! networks; the documented remedy is to retry with an authenticated browser
//! session. [`crate::cookies::CookieJar::parse_netscape`] loads a browser
//! cookie export and [`crate::cookies::CookieTransport`] wraps any
//! [`HttpClient`], attaching a `Cookie` header —
//! and, when a `SAPISID` cookie is present, the `SAPISIDHASH` `Authorization`
//! header InnerTube requires of logged-in callers — to requests whose host the
//! cookies' domains match.

use sha1::{Digest, Sha1};

use crate::error::{Error, Result};
use crate::transport::{HttpClient, HttpRequest, HttpResponse};

/// A single cookie from a Netscape-format export.
#[derive(Debug, Clone)]
pub struct Cookie {
    /// Cookie domain, lowercase, without any leading dot.
    pub domain: String,
    /// Whether subdomains of `domain` also match.
    pub include_subdomains: bool,
    /// Only send over HTTPS.
    pub secure: bool,
    /// Unix expiry in seconds; `0` means a session cookie (never expires here).
    pub expires: u64,
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
}

impl Cookie {
    /// Whether this cookie applies to `host` over the given scheme at `now`.
    fn matches(&self, host: &str, https: bool, now: u64) -> bool {
        if self.secure && !https {
            return false;
        }
        if self.expires != 0 && self.expires < now {
            return false;
        }
        let host = host.to_lowercase();
        host == self.domain
            || (self.include_subdomains && host.ends_with(&format!(".{}", self.domain)))
    }
}

/// An immutable set of cookies parsed from a Netscape `cookies.txt` file.
#[derive(Debug, Clone, Default)]
pub struct CookieJar {
    cookies: Vec<Cookie>,
}

impl CookieJar {
    /// Parse a Netscape-format `cookies.txt` (as exported by browsers and
    /// browser extensions, or by `yt-dlp --cookies`).
    ///
    /// Blank lines and comments are skipped; `#HttpOnly_`-prefixed entries are
    /// honored. Any other line must have the seven tab-separated fields of the
    /// format, or an [`Error::Extraction`] naming the line is returned.
    pub fn parse_netscape(text: &str) -> Result<Self> {
        let mut cookies = Vec::new();
        for (idx, raw) in text.lines().enumerate() {
            let line = raw.strip_prefix("#HttpOnly_").unwrap_or(raw);
            if line.trim().is_empty() || (line.starts_with('#') && !raw.starts_with("#HttpOnly_")) {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() != 7 {
                return Err(Error::Extraction {
                    stage: "cookies",
                    message: format!(
                        "line {}: expected 7 tab-separated fields (Netscape cookies.txt), got {}",
                        idx + 1,
                        fields.len()
                    ),
                });
            }
            let domain = fields[0].trim_start_matches('.').to_lowercase();
            cookies.push(Cookie {
                include_subdomains: fields[0].starts_with('.')
                    || fields[1].eq_ignore_ascii_case("TRUE"),
                domain,
                secure: fields[3].eq_ignore_ascii_case("TRUE"),
                expires: fields[4].parse().unwrap_or(0),
                name: fields[5].to_string(),
                value: fields[6].to_string(),
            });
        }
        Ok(Self { cookies })
    }

    /// Whether the jar holds no cookies.
    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    /// The `Cookie` header value for `host`, or `None` if nothing matches.
    pub fn header_for(&self, host: &str, https: bool, now: u64) -> Option<String> {
        let parts: Vec<String> = self
            .cookies
            .iter()
            .filter(|c| c.matches(host, https, now))
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("; "))
        }
    }

    /// The value of the cookie `name` applicable to `host`, if any.
    pub fn get(&self, host: &str, name: &str, https: bool, now: u64) -> Option<&str> {
        self.cookies
            .iter()
            .find(|c| c.name == name && c.matches(host, https, now))
            .map(|c| c.value.as_str())
    }
}

/// Compute the `SAPISIDHASH` `Authorization` header value Google endpoints
/// require of cookie-authenticated requests:
/// `SAPISIDHASH {now}_{sha1("{now} {sapisid} {origin}")}`.
pub fn sapisid_hash(sapisid: &str, origin: &str, now: u64) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{now} {sapisid} {origin}").as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("SAPISIDHASH {now}_{hex}")
}

/// Current Unix time in whole seconds.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// [`HttpClient`] decorator that attaches cookies (and `SAPISIDHASH`
/// authorization where applicable) to requests whose host matches the jar.
pub struct CookieTransport {
    inner: std::sync::Arc<dyn HttpClient>,
    jar: CookieJar,
}

impl CookieTransport {
    /// Wrap `inner`, attaching cookies from `jar` to matching requests.
    pub fn new(inner: std::sync::Arc<dyn HttpClient>, jar: CookieJar) -> Self {
        Self { inner, jar }
    }
}

#[async_trait::async_trait]
impl HttpClient for CookieTransport {
    async fn execute(&self, mut req: HttpRequest) -> Result<HttpResponse> {
        if let Ok(parsed) = url::Url::parse(&req.url) {
            if let Some(host) = parsed.host_str() {
                let https = parsed.scheme() == "https";
                let now = unix_now();
                if let Some(header) = self.jar.header_for(host, https, now) {
                    req.headers.push(("Cookie".into(), header));
                    // Logged-in InnerTube calls are only accepted alongside a
                    // SAPISIDHASH Authorization bound to the request origin.
                    let sapisid = self
                        .jar
                        .get(host, "SAPISID", https, now)
                        .or_else(|| self.jar.get(host, "__Secure-3PAPISID", https, now));
                    if let Some(sapisid) = sapisid {
                        let origin = parsed.origin().ascii_serialization();
                        req.headers
                            .push(("Authorization".into(), sapisid_hash(sapisid, &origin, now)));
                        req.headers.push(("X-Origin".into(), origin));
                    }
                    // Meta (Instagram) endpoints require cookie-authenticated
                    // requests to echo the csrftoken cookie as X-CSRFToken.
                    if let Some(csrf) = self.jar.get(host, "csrftoken", https, now) {
                        req.headers.push(("X-CSRFToken".into(), csrf.to_string()));
                    }
                }
            }
        }
        self.inner.execute(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAR: &str = "# Netscape HTTP Cookie File\n\
        \n\
        .youtube.com\tTRUE\t/\tTRUE\t253402300799\tSAPISID\tabc123\n\
        #HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t253402300799\tSID\tsid-value\n\
        .youtube.com\tTRUE\t/\tTRUE\t1\tEXPIRED\tgone\n\
        example.com\tFALSE\t/\tFALSE\t0\tPLAIN\tv\n";

    #[test]
    fn parses_and_matches_domains() {
        let jar = CookieJar::parse_netscape(JAR).unwrap();
        let header = jar.header_for("www.youtube.com", true, 1000).unwrap();
        assert!(header.contains("SAPISID=abc123"));
        // HttpOnly-prefixed entries are real cookies.
        assert!(header.contains("SID=sid-value"));
        // Expired cookies are dropped.
        assert!(!header.contains("EXPIRED"));
        // Host-only cookies do not leak to subdomains…
        assert!(jar.header_for("sub.example.com", false, 1000).is_none());
        // …but match their own host, and session cookies (expiry 0) survive.
        assert_eq!(
            jar.header_for("example.com", false, 1000).as_deref(),
            Some("PLAIN=v")
        );
        // Secure cookies never travel over plain HTTP.
        assert!(jar.header_for("www.youtube.com", false, 1000).is_none());
    }

    #[test]
    fn malformed_line_is_reported_with_its_number() {
        let err = CookieJar::parse_netscape("not a cookie line").unwrap_err();
        assert!(err.to_string().contains("line 1"));
    }

    #[test]
    fn sapisid_hash_is_deterministic() {
        // sha1("1700000000 abc123 https://www.youtube.com")
        let h = sapisid_hash("abc123", "https://www.youtube.com", 1_700_000_000);
        assert!(h.starts_with("SAPISIDHASH 1700000000_"));
        let hex = h.rsplit('_').next().unwrap();
        assert_eq!(hex.len(), 40);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    #[tokio::test]
    async fn attaches_cookie_and_sapisidhash_to_matching_host() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .and(|req: &Request| {
                let header = |name: &str| {
                    req.headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string)
                };
                header("cookie").as_deref() == Some("SAPISID=abc123")
                    && header("authorization").is_some_and(|a| a.starts_with("SAPISIDHASH "))
                    && header("x-origin").is_some_and(|o| o.starts_with("http://127.0.0.1"))
            })
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        // The wiremock host is 127.0.0.1, so bind the cookies to it (secure=
        // FALSE: the mock serves plain HTTP).
        let jar = CookieJar::parse_netscape(
            "127.0.0.1\tFALSE\t/\tFALSE\t0\tSAPISID\tabc123\n\
             other.example\tFALSE\t/\tFALSE\t0\tUNRELATED\tnope\n",
        )
        .unwrap();
        let client = CookieTransport::new(
            std::sync::Arc::new(crate::transport::ReqwestClient::new(reqwest::Client::new())),
            jar,
        );
        // A missing/mismatched Cookie or Authorization header would 404.
        let resp = client
            .execute(HttpRequest::get("test", format!("{}/x", server.uri())))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
    }

    /// Instagram's GraphQL endpoints reject cookie-authenticated requests that
    /// do not echo the `csrftoken` cookie as `X-CSRFToken`.
    #[tokio::test]
    async fn echoes_csrftoken_cookie_as_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql/query"))
            .and(|req: &Request| {
                req.headers.get("x-csrftoken").and_then(|v| v.to_str().ok()) == Some("tok123")
            })
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let jar = CookieJar::parse_netscape("127.0.0.1\tFALSE\t/\tFALSE\t0\tcsrftoken\ttok123\n")
            .unwrap();
        let client = CookieTransport::new(
            std::sync::Arc::new(crate::transport::ReqwestClient::new(reqwest::Client::new())),
            jar,
        );
        let resp = client
            .execute(HttpRequest::post(
                "test",
                format!("{}/graphql/query", server.uri()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status, 200, "X-CSRFToken header was not attached");
    }
}
