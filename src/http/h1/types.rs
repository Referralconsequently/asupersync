//! HTTP/1.1 protocol types.
//!
//! Provides [`Method`], [`Version`], and request/response types for HTTP/1.1
//! protocol handling.

use std::fmt;
use std::net::SocketAddr;

/// HTTP request method.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    /// GET
    Get,
    /// HEAD
    Head,
    /// POST
    Post,
    /// PUT
    Put,
    /// DELETE
    Delete,
    /// CONNECT
    Connect,
    /// OPTIONS
    Options,
    /// TRACE
    Trace,
    /// PATCH
    Patch,
    /// Extension method not covered by the standard set.
    Extension(String),
}

impl Method {
    /// Parse a method from its ASCII representation.
    #[must_use]
    pub fn from_bytes(src: &[u8]) -> Option<Self> {
        match src {
            b"GET" => Some(Self::Get),
            b"HEAD" => Some(Self::Head),
            b"POST" => Some(Self::Post),
            b"PUT" => Some(Self::Put),
            b"DELETE" => Some(Self::Delete),
            b"CONNECT" => Some(Self::Connect),
            b"OPTIONS" => Some(Self::Options),
            b"TRACE" => Some(Self::Trace),
            b"PATCH" => Some(Self::Patch),
            other => std::str::from_utf8(other)
                .ok()
                .map(|s| Self::Extension(s.to_owned())),
        }
    }

    /// Returns the method as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Connect => "CONNECT",
            Self::Options => "OPTIONS",
            Self::Trace => "TRACE",
            Self::Patch => "PATCH",
            Self::Extension(s) => s,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// HTTP version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    /// HTTP/1.0
    Http10,
    /// HTTP/1.1
    Http11,
}

impl Version {
    /// Parse a version from its ASCII representation (e.g. `HTTP/1.1`).
    #[must_use]
    pub fn from_bytes(src: &[u8]) -> Option<Self> {
        match src {
            b"HTTP/1.0" => Some(Self::Http10),
            b"HTTP/1.1" => Some(Self::Http11),
            _ => None,
        }
    }

    /// Returns the version as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Http10 => "HTTP/1.0",
            Self::Http11 => "HTTP/1.1",
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parsed HTTP/1.1 request (request line + headers + body).
#[derive(Debug, Clone)]
pub struct Request {
    /// HTTP method (GET, POST, etc.).
    pub method: Method,
    /// Request URI (e.g. `/path?query`).
    pub uri: String,
    /// HTTP version.
    pub version: Version,
    /// Request headers as name-value pairs.
    pub headers: Vec<(String, String)>,
    /// Request body bytes.
    pub body: Vec<u8>,
    /// Trailing headers (only valid for chunked transfer-encoding).
    pub trailers: Vec<(String, String)>,
    /// Remote peer address for the connection (if known).
    pub peer_addr: Option<SocketAddr>,
}

impl Request {
    /// Create a request builder for the provided method and URI.
    #[must_use]
    pub fn builder(method: Method, uri: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(method, uri)
    }

    /// Create a `GET` request builder.
    #[must_use]
    pub fn get(uri: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::Get, uri)
    }

    /// Create a `POST` request builder.
    #[must_use]
    pub fn post(uri: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::Post, uri)
    }

    /// Create a `PUT` request builder.
    #[must_use]
    pub fn put(uri: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::Put, uri)
    }

    /// Create a `DELETE` request builder.
    #[must_use]
    pub fn delete(uri: impl Into<String>) -> RequestBuilder {
        RequestBuilder::new(Method::Delete, uri)
    }
}

/// Fluent builder for [`Request`].
#[derive(Debug, Clone)]
pub struct RequestBuilder {
    request: Request,
}

impl RequestBuilder {
    /// Create a builder with HTTP/1.1 defaults.
    #[must_use]
    pub fn new(method: Method, uri: impl Into<String>) -> Self {
        Self {
            request: Request {
                method,
                uri: uri.into(),
                version: Version::Http11,
                headers: Vec::new(),
                body: Vec::new(),
                trailers: Vec::new(),
                peer_addr: None,
            },
        }
    }

    /// Set the request method.
    #[must_use]
    pub fn method(mut self, method: Method) -> Self {
        self.request.method = method;
        self
    }

    /// Set the request URI.
    #[must_use]
    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.request.uri = uri.into();
        self
    }

    /// Set the HTTP version.
    #[must_use]
    pub fn version(mut self, version: Version) -> Self {
        self.request.version = version;
        self
    }

    /// Add a header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.request.headers.push((name.into(), value.into()));
        self
    }

    /// Add multiple headers.
    #[must_use]
    pub fn headers<I, N, V>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (N, V)>,
        N: Into<String>,
        V: Into<String>,
    {
        self.request.headers.extend(
            headers
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        );
        self
    }

    /// Set request body bytes.
    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.request.body = body.into();
        self
    }

    /// Add a trailer header.
    #[must_use]
    pub fn trailer(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.request.trailers.push((name.into(), value.into()));
        self
    }

    /// Set remote peer address metadata.
    #[must_use]
    pub fn peer_addr(mut self, peer_addr: SocketAddr) -> Self {
        self.request.peer_addr = Some(peer_addr);
        self
    }

    /// Build the request.
    #[must_use]
    pub fn build(self) -> Request {
        self.request
    }
}

/// Parsed HTTP/1.1 response (status line + headers + body).
#[derive(Debug, Clone)]
pub struct Response {
    /// HTTP version.
    pub version: Version,
    /// Status code (e.g. 200, 404).
    pub status: u16,
    /// Reason phrase (e.g. "OK", "Not Found").
    pub reason: String,
    /// Response headers as name-value pairs.
    pub headers: Vec<(String, String)>,
    /// Response body bytes.
    pub body: Vec<u8>,
    /// Trailing headers (only valid for chunked transfer-encoding).
    pub trailers: Vec<(String, String)>,
}

impl Response {
    /// Create a simple response with the given status, reason, and body.
    #[must_use]
    pub fn new(status: u16, reason: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            version: Version::Http11,
            status,
            reason: reason.into(),
            headers: Vec::new(),
            body: body.into(),
            trailers: Vec::new(),
        }
    }

    /// Add a header.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Create a response builder using the standard reason phrase for `status`.
    #[must_use]
    pub fn builder(status: u16) -> ResponseBuilder {
        ResponseBuilder::new(status)
    }

    /// Add a trailer header.
    ///
    /// Trailers are only valid with `Transfer-Encoding: chunked`.
    #[must_use]
    pub fn with_trailer(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.trailers.push((name.into(), value.into()));
        self
    }
}

/// Fluent builder for [`Response`].
#[derive(Debug, Clone)]
pub struct ResponseBuilder {
    response: Response,
}

impl ResponseBuilder {
    /// Create a builder with HTTP/1.1 defaults and the standard reason phrase.
    #[must_use]
    pub fn new(status: u16) -> Self {
        Self {
            response: Response {
                version: Version::Http11,
                status,
                reason: default_reason(status).to_owned(),
                headers: Vec::new(),
                body: Vec::new(),
                trailers: Vec::new(),
            },
        }
    }

    /// Set response status and reset reason phrase to the default for that code.
    #[must_use]
    pub fn status(mut self, status: u16) -> Self {
        self.response.status = status;
        self.response.reason = default_reason(status).to_owned();
        self
    }

    /// Set response reason phrase.
    #[must_use]
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.response.reason = reason.into();
        self
    }

    /// Set HTTP version.
    #[must_use]
    pub fn version(mut self, version: Version) -> Self {
        self.response.version = version;
        self
    }

    /// Add a header.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.response.headers.push((name.into(), value.into()));
        self
    }

    /// Add multiple headers.
    #[must_use]
    pub fn headers<I, N, V>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (N, V)>,
        N: Into<String>,
        V: Into<String>,
    {
        self.response.headers.extend(
            headers
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        );
        self
    }

    /// Set response body bytes.
    #[must_use]
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.response.body = body.into();
        self
    }

    /// Add a trailer header.
    #[must_use]
    pub fn trailer(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.response.trailers.push((name.into(), value.into()));
        self
    }

    /// Build the response.
    #[must_use]
    pub fn build(self) -> Response {
        self.response
    }
}

/// Returns the standard reason phrase for a status code.
#[must_use]
pub fn default_reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        417 => "Expectation Failed",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_roundtrip() {
        for (bytes, expected) in [
            (&b"GET"[..], Method::Get),
            (b"POST", Method::Post),
            (b"DELETE", Method::Delete),
            (b"PATCH", Method::Patch),
            (b"CUSTOM", Method::Extension("CUSTOM".into())),
        ] {
            let parsed = Method::from_bytes(bytes).unwrap();
            assert_eq!(parsed, expected);
            let reparsed = Method::from_bytes(parsed.as_str().as_bytes()).unwrap();
            assert_eq!(reparsed, expected);
        }
    }

    #[test]
    fn version_roundtrip() {
        assert_eq!(Version::from_bytes(b"HTTP/1.0"), Some(Version::Http10));
        assert_eq!(Version::from_bytes(b"HTTP/1.1"), Some(Version::Http11));
        assert_eq!(Version::from_bytes(b"HTTP/2"), None);
        assert_eq!(Version::Http11.as_str(), "HTTP/1.1");
    }

    #[test]
    fn response_builder() {
        let resp =
            Response::new(200, "OK", b"hello".to_vec()).with_header("Content-Type", "text/plain");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.headers.len(), 1);
        assert_eq!(resp.body, b"hello");
        assert!(resp.trailers.is_empty());
    }

    #[test]
    fn default_reasons() {
        assert_eq!(default_reason(200), "OK");
        assert_eq!(default_reason(404), "Not Found");
        assert_eq!(default_reason(417), "Expectation Failed");
        assert_eq!(default_reason(500), "Internal Server Error");
        assert_eq!(default_reason(999), "Unknown");
    }

    // Pure data-type tests (wave 12 – CyanBarn)

    #[test]
    fn method_display_all_standard() {
        assert_eq!(Method::Get.to_string(), "GET");
        assert_eq!(Method::Head.to_string(), "HEAD");
        assert_eq!(Method::Post.to_string(), "POST");
        assert_eq!(Method::Put.to_string(), "PUT");
        assert_eq!(Method::Delete.to_string(), "DELETE");
        assert_eq!(Method::Connect.to_string(), "CONNECT");
        assert_eq!(Method::Options.to_string(), "OPTIONS");
        assert_eq!(Method::Trace.to_string(), "TRACE");
        assert_eq!(Method::Patch.to_string(), "PATCH");
    }

    #[test]
    fn method_display_extension() {
        let ext = Method::Extension("PURGE".into());
        assert_eq!(ext.to_string(), "PURGE");
    }

    #[test]
    fn method_debug_clone_eq_hash() {
        use std::collections::HashSet;

        let m = Method::Get;
        let dbg = format!("{m:?}");
        assert!(dbg.contains("Get"));
        let cloned = m.clone();
        assert_eq!(m, cloned);

        let mut set = HashSet::new();
        set.insert(Method::Get);
        set.insert(Method::Post);
        set.insert(Method::Get);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn method_from_bytes_all_standard() {
        let methods = [
            (b"GET" as &[u8], Method::Get),
            (b"HEAD", Method::Head),
            (b"POST", Method::Post),
            (b"PUT", Method::Put),
            (b"DELETE", Method::Delete),
            (b"CONNECT", Method::Connect),
            (b"OPTIONS", Method::Options),
            (b"TRACE", Method::Trace),
            (b"PATCH", Method::Patch),
        ];
        for (bytes, expected) in methods {
            assert_eq!(Method::from_bytes(bytes), Some(expected));
        }
    }

    #[test]
    fn method_from_bytes_invalid_utf8() {
        // Invalid UTF-8 should return None (not an extension)
        assert!(Method::from_bytes(&[0xFF, 0xFE]).is_none());
    }

    #[test]
    fn method_inequality() {
        assert_ne!(Method::Get, Method::Post);
        assert_ne!(Method::Get, Method::Extension("GET".into()));
    }

    #[test]
    fn version_display() {
        assert_eq!(Version::Http10.to_string(), "HTTP/1.0");
        assert_eq!(Version::Http11.to_string(), "HTTP/1.1");
    }

    #[test]
    fn version_debug_copy_eq_hash() {
        use std::collections::HashSet;

        let v = Version::Http11;
        let dbg = format!("{v:?}");
        assert!(dbg.contains("Http11"));
        let copied = v;
        assert_eq!(v, copied);

        let mut set = HashSet::new();
        set.insert(Version::Http10);
        set.insert(Version::Http11);
        set.insert(Version::Http10);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn request_debug_clone() {
        let req = Request {
            method: Method::Get,
            uri: "/path".to_string(),
            version: Version::Http11,
            headers: vec![("Host".to_string(), "example.com".to_string())],
            body: b"body".to_vec(),
            trailers: vec![],
            peer_addr: None,
        };
        let dbg = format!("{req:?}");
        assert!(dbg.contains("Get"));
        assert!(dbg.contains("/path"));

        let cloned = req;
        assert_eq!(cloned.method, Method::Get);
        assert_eq!(cloned.uri, "/path");
        assert_eq!(cloned.headers.len(), 1);
    }

    #[test]
    fn request_with_peer_addr() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let req = Request {
            method: Method::Post,
            uri: "/api".to_string(),
            version: Version::Http11,
            headers: vec![],
            body: vec![],
            trailers: vec![],
            peer_addr: Some(addr),
        };
        assert_eq!(req.peer_addr, Some(addr));
    }

    #[test]
    fn request_builder_sets_fields() {
        let peer_addr: SocketAddr = "10.0.0.9:9000".parse().unwrap();
        let req = Request::builder(Method::Patch, "/v1/items/7")
            .version(Version::Http10)
            .header("Host", "example.com")
            .header("X-Trace-Id", "abc123")
            .body(b"payload".to_vec())
            .trailer("Checksum", "sha256:deadbeef")
            .peer_addr(peer_addr)
            .build();

        assert_eq!(req.method, Method::Patch);
        assert_eq!(req.uri, "/v1/items/7");
        assert_eq!(req.version, Version::Http10);
        assert_eq!(
            req.headers,
            vec![
                ("Host".to_string(), "example.com".to_string()),
                ("X-Trace-Id".to_string(), "abc123".to_string()),
            ]
        );
        assert_eq!(req.body, b"payload");
        assert_eq!(
            req.trailers,
            vec![("Checksum".to_string(), "sha256:deadbeef".to_string())]
        );
        assert_eq!(req.peer_addr, Some(peer_addr));
    }

    #[test]
    fn request_convenience_builders_use_expected_method() {
        let get_req = Request::get("/health").build();
        assert_eq!(get_req.method, Method::Get);
        assert_eq!(get_req.uri, "/health");
        assert_eq!(get_req.version, Version::Http11);

        let post_req = Request::post("/submit").build();
        assert_eq!(post_req.method, Method::Post);
        assert_eq!(post_req.uri, "/submit");
        assert_eq!(post_req.version, Version::Http11);

        let put_req = Request::put("/resource/1").build();
        assert_eq!(put_req.method, Method::Put);

        let delete_req = Request::delete("/resource/1").build();
        assert_eq!(delete_req.method, Method::Delete);
    }

    #[test]
    fn response_with_trailer() {
        let resp = Response::new(200, "OK", Vec::<u8>::new())
            .with_header("Transfer-Encoding", "chunked")
            .with_trailer("Checksum", "abc123");
        assert_eq!(resp.trailers.len(), 1);
        assert_eq!(resp.trailers[0].0, "Checksum");
        assert_eq!(resp.trailers[0].1, "abc123");
    }

    #[test]
    fn response_debug_clone() {
        let resp = Response::new(404, "Not Found", b"missing".to_vec());
        let dbg = format!("{resp:?}");
        assert!(dbg.contains("404"));
        let cloned = resp;
        assert_eq!(cloned.status, 404);
        assert_eq!(cloned.reason, "Not Found");
    }

    #[test]
    fn response_defaults_version_http11() {
        let resp = Response::new(200, "OK", Vec::<u8>::new());
        assert_eq!(resp.version, Version::Http11);
    }

    #[test]
    fn response_builder_uses_default_reason_and_chainable_fields() {
        let resp = Response::builder(201)
            .header("Content-Type", "application/json")
            .body(br#"{"ok":true}"#.to_vec())
            .trailer("Checksum", "abc123")
            .build();

        assert_eq!(resp.version, Version::Http11);
        assert_eq!(resp.status, 201);
        assert_eq!(resp.reason, "Created");
        assert_eq!(
            resp.headers,
            vec![("Content-Type".to_string(), "application/json".to_string())]
        );
        assert_eq!(resp.body, br#"{"ok":true}"#);
        assert_eq!(
            resp.trailers,
            vec![("Checksum".to_string(), "abc123".to_string())]
        );
    }

    #[test]
    fn response_builder_status_resets_reason_unless_overridden_afterward() {
        let resp = Response::builder(200)
            .reason("Everything Fine")
            .status(404)
            .build();
        assert_eq!(resp.status, 404);
        assert_eq!(resp.reason, "Not Found");

        let resp_with_custom_reason = Response::builder(200)
            .status(503)
            .reason("Service Busy")
            .build();
        assert_eq!(resp_with_custom_reason.status, 503);
        assert_eq!(resp_with_custom_reason.reason, "Service Busy");
    }

    #[test]
    fn default_reason_all_known() {
        let known = [
            (100, "Continue"),
            (201, "Created"),
            (204, "No Content"),
            (301, "Moved Permanently"),
            (302, "Found"),
            (304, "Not Modified"),
            (400, "Bad Request"),
            (401, "Unauthorized"),
            (403, "Forbidden"),
            (405, "Method Not Allowed"),
            (408, "Request Timeout"),
            (411, "Length Required"),
            (413, "Payload Too Large"),
            (414, "URI Too Long"),
            (431, "Request Header Fields Too Large"),
            (501, "Not Implemented"),
            (502, "Bad Gateway"),
            (503, "Service Unavailable"),
        ];
        for (code, expected) in known {
            assert_eq!(default_reason(code), expected, "code={code}");
        }
    }
}
