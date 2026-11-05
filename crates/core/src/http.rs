//! A tiny HTTP abstraction so providers can be tested against canned responses.

use std::io::{Read, Write};
use std::time::Duration;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct Response {
    pub status: u16,
    /// Header names are lowercased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }
}

pub trait Http {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response>;
    fn head(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response>;
    /// Stream the body of `url` into `out`, starting at byte `offset` (a resume). If the server
    /// does not honour the range, nothing is written and `Error::RangeIgnored` is returned.
    /// `progress(done, total)` counts from the start of the file, not from `offset`.
    fn download(
        &self,
        url: &str,
        offset: u64,
        out: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()>;
}

pub struct UreqHttp {
    agent: ureq::Agent,
}

/// Small API answers (release lists, checksums) must arrive within this long.
const API_BODY_TIMEOUT: Duration = Duration::from_secs(60);
/// A download that receives nothing for this long has stalled and is abandoned. There is no
/// limit on the total time, so slow links still finish; the partial file is kept for a resume.
const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(60);

const USER_AGENT: &str = concat!("appofholding/", env!("CARGO_PKG_VERSION"));

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqHttp {
    pub fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .user_agent(USER_AGENT)
            .build()
            .into();
        Self { agent }
    }

    fn token_for(url: &str) -> Option<String> {
        // Only ever sent to the GitHub API host, never to asset CDNs or other servers.
        if url.starts_with("https://api.github.com/") {
            std::env::var("GITHUB_TOKEN")
                .or_else(|_| std::env::var("GH_TOKEN"))
                .ok()
                .filter(|t| !t.is_empty())
        } else {
            None
        }
    }

    fn convert(resp: ureq::http::Response<ureq::Body>, with_body: bool) -> Result<Response> {
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                Some((
                    k.as_str().to_ascii_lowercase(),
                    v.to_str().ok()?.to_string(),
                ))
            })
            .collect();
        // 304 and 204 never have a body. Servers still label a 304 with the original
        // `Content-Encoding: gzip`, and trying to decompress nothing is an error: GitHub's
        // "not modified" answers failed this way.
        let body = if with_body && status != 304 && status != 204 {
            resp.into_body()
                .with_config()
                .limit(32 << 20)
                .read_to_vec()
                .map_err(|e| Error::Http(e.to_string()))?
        } else {
            Vec::new()
        };
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

impl Http for UreqHttp {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response> {
        let mut req = self
            .agent
            .get(url)
            .config()
            .timeout_recv_body(Some(API_BODY_TIMEOUT))
            .build();
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        if let Some(t) = Self::token_for(url) {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        Self::convert(req.call().map_err(|e| Error::Http(e.to_string()))?, true)
    }

    fn head(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response> {
        let mut req = self.agent.head(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        Self::convert(req.call().map_err(|e| Error::Http(e.to_string()))?, false)
    }

    fn download(
        &self,
        url: &str,
        offset: u64,
        out: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()> {
        let mut req = self
            .agent
            .get(url)
            .config()
            .timeout_per_read(Some(DOWNLOAD_STALL_TIMEOUT))
            .build();
        if offset > 0 {
            req = req.header("Range", format!("bytes={offset}-"));
        }
        let resp = req.call().map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        if offset > 0 && (status == 200 || status == 416) {
            return Err(Error::RangeIgnored);
        }
        if !(200..300).contains(&status) {
            return Err(Error::Http(format!(
                "download failed: HTTP {status} for {url}"
            )));
        }
        let total = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|len| len + offset);
        let mut body = resp.into_body();
        let mut reader = body.as_reader();
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = offset;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| Error::Http(e.to_string()))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|e| Error::Http(e.to_string()))?;
            done += n as u64;
            progress(done, total);
        }
        Ok(())
    }
}

/// Canned-response HTTP client for tests.
#[cfg(any(test, feature = "fixtures"))]
pub mod fake {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    pub struct FakeHttp {
        pub responses: HashMap<String, Response>,
        pub requests: Mutex<Vec<String>>,
        /// Pretend the server does not support Range requests.
        pub ignore_range: bool,
        /// Drop the connection after this many body bytes (first attempt semantics: applies to
        /// every download call).
        pub fail_after: Option<usize>,
    }

    impl FakeHttp {
        pub fn json(mut self, url: &str, body: &str) -> Self {
            self.responses.insert(
                url.to_string(),
                Response {
                    status: 200,
                    headers: vec![],
                    body: body.as_bytes().to_vec(),
                },
            );
            self
        }
        pub fn status(mut self, url: &str, status: u16, headers: &[(&str, &str)]) -> Self {
            self.responses.insert(
                url.to_string(),
                Response {
                    status,
                    headers: headers
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                    body: vec![],
                },
            );
            self
        }
        pub fn bytes(mut self, url: &str, body: Vec<u8>) -> Self {
            self.responses.insert(
                url.to_string(),
                Response {
                    status: 200,
                    headers: vec![],
                    body,
                },
            );
            self
        }
        fn lookup(&self, url: &str) -> Result<Response> {
            self.requests.lock().unwrap().push(url.to_string());
            self.responses
                .get(url)
                .cloned()
                .ok_or_else(|| Error::Http(format!("no canned response for {url}")))
        }
    }

    impl Http for FakeHttp {
        fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response> {
            let r = self.lookup(url)?;
            // Behave like a server that supports conditional requests.
            let inm = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("if-none-match"));
            if let (Some((_, want)), Some(have)) = (inm, r.header("etag"))
                && *want == have
            {
                return Ok(Response {
                    status: 304,
                    headers: r.headers.clone(),
                    body: vec![],
                });
            }
            Ok(r)
        }
        fn head(&self, url: &str, _: &[(&str, &str)]) -> Result<Response> {
            let mut r = self.lookup(url)?;
            r.body.clear();
            Ok(r)
        }
        fn download(
            &self,
            url: &str,
            offset: u64,
            out: &mut dyn Write,
            progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<()> {
            let r = self.lookup(url)?;
            if r.status != 200 {
                return Err(Error::Http(format!("HTTP {}", r.status)));
            }
            if offset > 0 && self.ignore_range {
                return Err(Error::RangeIgnored);
            }
            let rest = &r.body[(offset as usize).min(r.body.len())..];
            // `fail_after` simulates a connection that drops mid-download.
            let n = self.fail_after.map_or(rest.len(), |f| f.min(rest.len()));
            out.write_all(&rest[..n])
                .map_err(|e| Error::Http(e.to_string()))?;
            if n < rest.len() {
                return Err(Error::Http("connection reset".into()));
            }
            progress(r.body.len() as u64, Some(r.body.len() as u64));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Serve one canned raw HTTP response on localhost and return the URL.
    fn serve_once(raw: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(raw);
            }
        });
        format!("http://{addr}/x")
    }

    #[test]
    fn a_304_labelled_gzip_has_no_body_to_decompress() {
        // Exactly what GitHub sends for a conditional request that matched.
        let url = serve_once(
            b"HTTP/1.1 304 Not Modified\r\nContent-Encoding: gzip\r\nETag: W/\"abc\"\r\nConnection: close\r\n\r\n",
        );
        let r = UreqHttp::new()
            .get(&url, &[("If-None-Match", "W/\"abc\"")])
            .unwrap();
        assert_eq!(r.status, 304);
        assert!(r.body.is_empty());
        assert_eq!(r.header("etag"), Some("W/\"abc\""));
    }

    #[test]
    fn ordinary_bodies_and_errors_still_work() {
        let url =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
        let r = UreqHttp::new().get(&url, &[]).unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, b"hello".as_slice()));
        let url =
            serve_once(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert_eq!(UreqHttp::new().get(&url, &[]).unwrap().status, 404);
    }
}
