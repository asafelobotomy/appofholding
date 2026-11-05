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
    /// Stream the body of `url` into `out`. `progress(done, total)` is called as data arrives.
    fn download(
        &self,
        url: &str,
        out: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()>;
}

pub struct UreqHttp {
    agent: ureq::Agent,
}

const USER_AGENT: &str = concat!("appimgr/", env!("CARGO_PKG_VERSION"));

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
        let body = if with_body {
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
        let mut req = self.agent.get(url);
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
        out: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()> {
        let resp = self
            .agent
            .get(url)
            .call()
            .map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(Error::Http(format!(
                "download failed: HTTP {status} for {url}"
            )));
        }
        let total = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let mut body = resp.into_body();
        let mut reader = body.as_reader();
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
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
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    pub struct FakeHttp {
        pub responses: HashMap<String, Response>,
        pub requests: RefCell<Vec<String>>,
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
            self.requests.borrow_mut().push(url.to_string());
            self.responses
                .get(url)
                .cloned()
                .ok_or_else(|| Error::Http(format!("no canned response for {url}")))
        }
    }

    impl Http for FakeHttp {
        fn get(&self, url: &str, _: &[(&str, &str)]) -> Result<Response> {
            self.lookup(url)
        }
        fn head(&self, url: &str, _: &[(&str, &str)]) -> Result<Response> {
            let mut r = self.lookup(url)?;
            r.body.clear();
            Ok(r)
        }
        fn download(
            &self,
            url: &str,
            out: &mut dyn Write,
            progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<()> {
            let r = self.lookup(url)?;
            if r.status != 200 {
                return Err(Error::Http(format!("HTTP {}", r.status)));
            }
            out.write_all(&r.body)
                .map_err(|e| Error::Http(e.to_string()))?;
            progress(r.body.len() as u64, Some(r.body.len() as u64));
            Ok(())
        }
    }
}
