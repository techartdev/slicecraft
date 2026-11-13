//! Printers on the network: upload G-code to an OctoPrint or Moonraker
//! (Klipper) host and optionally start it, check that a host answers, and
//! fetch a camera snapshot.
//!
//! Plain HTTP/1.1 over a TCP socket, from the hosts' public API documentation:
//! OctoPrint `POST /api/files/local` and `GET /api/version` with an `X-Api-Key`
//! header; Moonraker `POST /server/files/upload` and `GET /server/info`. HTTPS
//! is refused with a clear error: printers are reached on the local network.
//! The API key is sent only in its header and never appears in an error.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Largest response read from a host.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
/// Largest G-code upload.
pub const MAX_UPLOAD_BYTES: usize = 512 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(120);

/// Which print host software answers at the address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostKind {
    #[default]
    OctoPrint,
    /// Klipper's API server (Mainsail and Fluidd talk to it).
    Moonraker,
}

impl HostKind {
    pub const ALL: [HostKind; 2] = [HostKind::OctoPrint, HostKind::Moonraker];

    pub fn name(self) -> &'static str {
        match self {
            HostKind::OctoPrint => "octoprint",
            HostKind::Moonraker => "moonraker",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HostKind::OctoPrint => "OctoPrint",
            HostKind::Moonraker => "Moonraker (Klipper)",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// A print host on the network.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct NetworkPrinter {
    pub kind: HostKind,
    /// `http://host[:port][/base]`, or just `host[:port]`.
    pub url: String,
    /// OctoPrint needs one; Moonraker only when it is configured to.
    pub api_key: String,
}

impl fmt::Debug for NetworkPrinter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // never print the key
        f.debug_struct("NetworkPrinter")
            .field("kind", &self.kind)
            .field("url", &self.url)
            .field("api_key", &if self.api_key.is_empty() { "" } else { "(set)" })
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    BadUrl(&'static str),
    Https,
    /// The host could not be reached.
    Connect(String),
    /// The host answered with an error status.
    Status {
        status: u16,
        message: String,
    },
    /// The answer was not HTTP we understand.
    Response(&'static str),
    TooLarge,
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::BadUrl(what) => write!(f, "bad printer address: {what}"),
            NetError::Https => f.write_str(
                "HTTPS addresses are not supported yet; use the printer's http:// address on the local network",
            ),
            NetError::Connect(e) => write!(f, "cannot reach the printer: {e}"),
            NetError::Status { status: 401 | 403, .. } => {
                f.write_str("the printer refused the request: check the API key")
            }
            NetError::Status { status, message } => {
                write!(f, "the printer answered {status}")?;
                if !message.is_empty() {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
            NetError::Response(what) => write!(f, "unexpected answer from the printer: {what}"),
            NetError::TooLarge => f.write_str("too much data for one transfer"),
        }
    }
}

impl std::error::Error for NetError {}

/// A parsed `http://` address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpUrl {
    pub host: String,
    pub port: u16,
    /// Path prefix without a trailing slash (`""` or `/octoprint`).
    pub path: String,
}

pub fn parse_url(s: &str) -> Result<HttpUrl, NetError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(NetError::BadUrl("it is empty"));
    }
    if s.len() > 2048 || s.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(NetError::BadUrl("it contains spaces or control characters"));
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Err(NetError::Https);
    }
    let rest = if lower.starts_with("http://") { s.get(7..).unwrap_or("") } else { s };
    if rest.contains("://") {
        return Err(NetError::BadUrl("only http:// addresses are supported"));
    }
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (rest.get(..i).unwrap_or(""), rest.get(i..).unwrap_or("")),
        None => (rest, ""),
    };
    if authority.contains('@') {
        return Err(NetError::BadUrl("user names in the address are not supported"));
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let Some(end) = v6.find(']') else { return Err(NetError::BadUrl("an unclosed [")) };
        let host = v6.get(..end).unwrap_or("");
        let after = v6.get(end + 1..).unwrap_or("");
        let port = match after.strip_prefix(':') {
            Some(p) => {
                p.parse::<u16>().map_err(|_| NetError::BadUrl("the port is not a number"))?
            }
            None if after.is_empty() => 80,
            None => return Err(NetError::BadUrl("text after the address")),
        };
        (host.to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (
                h.to_string(),
                p.parse::<u16>().map_err(|_| NetError::BadUrl("the port is not a number"))?,
            ),
            None => (authority.to_string(), 80),
        }
    };
    if host.is_empty() || port == 0 {
        return Err(NetError::BadUrl("no host"));
    }
    let path = path.split(['?', '#']).next().unwrap_or("").trim_end_matches('/').to_string();
    Ok(HttpUrl { host, port, path })
}

/// One HTTP answer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Send one request and read the whole answer (the connection is closed after).
pub fn request(
    url: &HttpUrl,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> Result<Response, NetError> {
    if body.len() > MAX_UPLOAD_BYTES {
        return Err(NetError::TooLarge);
    }
    let addrs: Vec<_> = (url.host.as_str(), url.port)
        .to_socket_addrs()
        .map_err(|e| NetError::Connect(format!("{}: {e}", url.host)))?
        .collect();
    let mut stream = None;
    let mut last = String::from("no address");
    for a in addrs.iter().take(8) {
        match TcpStream::connect_timeout(a, CONNECT_TIMEOUT) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = format!("{}:{}: {e}", url.host, url.port),
        }
    }
    let Some(mut stream) = stream else { return Err(NetError::Connect(last)) };
    let io = |e: std::io::Error| NetError::Connect(e.to_string());
    stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(io)?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(io)?;
    let host = if url.host.contains(':') { format!("[{}]", url.host) } else { url.host.clone() };
    let mut head = format!(
        "{method} {}{path} HTTP/1.1\r\nHost: {host}:{}\r\nConnection: close\r\nUser-Agent: SliceCraft\r\nAccept: */*\r\nContent-Length: {}\r\n",
        url.path,
        url.port,
        body.len()
    );
    for (k, v) in headers {
        if v.contains(['\r', '\n']) {
            return Err(NetError::BadUrl("a header value contains a line break"));
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).map_err(io)?;
    for chunk in body.chunks(64 * 1024) {
        stream.write_all(chunk).map_err(io)?;
    }
    stream.flush().map_err(io)?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            // a host that closes without a clean shutdown: keep what came
            Err(_) if !raw.is_empty() => break,
            Err(e) => return Err(io(e)),
        };
        if raw.len() + n > MAX_RESPONSE_BYTES {
            return Err(NetError::TooLarge);
        }
        raw.extend_from_slice(buf.get(..n).unwrap_or(&[]));
    }
    parse_response(&raw)
}

/// Split a raw HTTP/1.x answer into status, headers and body (chunked bodies are joined).
pub fn parse_response(raw: &[u8]) -> Result<Response, NetError> {
    let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err(NetError::Response("no end of headers"));
    };
    let head = String::from_utf8_lossy(raw.get(..end).unwrap_or(&[])).into_owned();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.split_whitespace();
    if !parts.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err(NetError::Response("not an HTTP answer"));
    }
    let status = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|s| (100..600).contains(s))
        .ok_or(NetError::Response("no status code"))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .take(200)
        .collect();
    let rest = raw.get(end + 4..).unwrap_or(&[]);
    let chunked = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")
    });
    let body = if chunked {
        dechunk(rest)?
    } else {
        let len = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, v)| v.parse::<usize>().ok());
        match len {
            Some(n) => rest.get(..n.min(rest.len())).unwrap_or(&[]).to_vec(),
            None => rest.to_vec(),
        }
    };
    Ok(Response { status, headers, body })
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, NetError> {
    let mut out = Vec::new();
    for _ in 0..1_000_000 {
        let Some(eol) = data.windows(2).position(|w| w == b"\r\n") else {
            return Err(NetError::Response("a chunk size line is cut off"));
        };
        let line = String::from_utf8_lossy(data.get(..eol).unwrap_or(&[])).into_owned();
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| NetError::Response("a bad chunk size"))?;
        data = data.get(eol + 2..).unwrap_or(&[]);
        if size == 0 {
            return Ok(out);
        }
        let Some(chunk) = data.get(..size) else {
            return Err(NetError::Response("a chunk is cut off"));
        };
        if out.len() + size > MAX_RESPONSE_BYTES {
            return Err(NetError::TooLarge);
        }
        out.extend_from_slice(chunk);
        data = data.get(size..).unwrap_or(&[]);
        data = data.strip_prefix(b"\r\n").unwrap_or(data);
    }
    Err(NetError::Response("too many chunks"))
}

fn ok(r: Response) -> Result<Response, NetError> {
    if (200..300).contains(&r.status) {
        Ok(r)
    } else {
        let text = r.text();
        let message: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
        Err(NetError::Status { status: r.status, message })
    }
}

fn key_header(p: &NetworkPrinter) -> Vec<(&'static str, String)> {
    let key = p.api_key.trim();
    if key.is_empty() { Vec::new() } else { vec![("X-Api-Key", key.to_string())] }
}

/// A file name the hosts accept: letters, digits, `-`, `_`, `.` and spaces, ending in `.gcode`.
pub fn safe_file_name(name: &str) -> String {
    let stem = name.trim().trim_end_matches(".gcode").trim_end_matches(".gco");
    let mut out: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_. ".contains(c) { c } else { '_' })
        .take(120)
        .collect();
    let trimmed = out.trim_matches(['.', ' ']).to_string();
    out = if trimmed.is_empty() { "slicecraft".into() } else { trimmed };
    out.push_str(".gcode");
    out
}

/// Ask the host who it is: a short description on success.
pub fn check(p: &NetworkPrinter) -> Result<String, NetError> {
    let url = parse_url(&p.url)?;
    let path = match p.kind {
        HostKind::OctoPrint => "/api/version",
        HostKind::Moonraker => "/server/info",
    };
    let r = ok(request(&url, "GET", path, &key_header(p), &[])?)?;
    let text = r.text();
    let field = |name: &str| -> Option<String> {
        let at = text.find(&format!("\"{name}\""))?;
        let after = text.get(at + name.len() + 2..)?;
        let value = after.trim_start().strip_prefix(':')?.trim_start();
        let value = value.strip_prefix('"')?;
        Some(value.split('"').next()?.chars().take(60).collect())
    };
    Ok(match p.kind {
        HostKind::OctoPrint => match field("text") {
            Some(t) => format!("{t} answers at {}:{}", url.host, url.port),
            None => format!("OctoPrint answers at {}:{}", url.host, url.port),
        },
        HostKind::Moonraker => match field("klippy_state") {
            Some(s) => format!("Moonraker answers at {}:{} (Klipper {s})", url.host, url.port),
            None => format!("Moonraker answers at {}:{}", url.host, url.port),
        },
    })
}

/// The multipart body for an upload, with its boundary.
pub fn multipart(file_name: &str, data: &[u8], fields: &[(&str, &str)]) -> (String, Vec<u8>) {
    // a boundary that cannot occur in the data
    let mut boundary = String::from("----SliceCraftBoundary7d1c");
    let mut n = 0u32;
    while data.windows(boundary.len()).any(|w| w == boundary.as_bytes()) && n < 64 {
        n += 1;
        boundary.push_str(&format!("{:x}", crate::threemf::crc32(boundary.as_bytes()) ^ n));
    }
    let mut body = Vec::with_capacity(data.len() + 512);
    for (k, v) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n")
                .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

/// Upload G-code as `file_name`; with `start`, the host prints it at once.
/// Returns what the host said, shortened.
pub fn upload(
    p: &NetworkPrinter,
    file_name: &str,
    gcode: &[u8],
    start: bool,
) -> Result<String, NetError> {
    let url = parse_url(&p.url)?;
    let name = safe_file_name(file_name);
    let flag = if start { "true" } else { "false" };
    let (path, fields): (&str, Vec<(&str, &str)>) = match p.kind {
        HostKind::OctoPrint => ("/api/files/local", vec![("select", flag), ("print", flag)]),
        HostKind::Moonraker => ("/server/files/upload", vec![("root", "gcodes"), ("print", flag)]),
    };
    let (boundary, body) = multipart(&name, gcode, &fields);
    let mut headers = key_header(p);
    headers.push(("Content-Type", format!("multipart/form-data; boundary={boundary}")));
    ok(request(&url, "POST", path, &headers, &body)?)?;
    Ok(if start {
        format!("{name} uploaded to {} and started", url.host)
    } else {
        format!("{name} uploaded to {}", url.host)
    })
}

/// Fetch one picture (a camera snapshot) from a full `http://` address.
pub fn fetch(address: &str, api_key: &str) -> Result<Vec<u8>, NetError> {
    let url = parse_url(address)?;
    // the parsed path is the whole path here; keep its query too
    let query = address.find('?').and_then(|i| address.get(i..)).unwrap_or("");
    let target = HttpUrl { path: String::new(), ..url.clone() };
    let path = format!("{}{}", if url.path.is_empty() { "/" } else { url.path.as_str() }, query);
    let key = api_key.trim();
    let headers = if key.is_empty() { Vec::new() } else { vec![("X-Api-Key", key.to_string())] };
    Ok(ok(request(&target, "GET", &path, &headers, &[])?)?.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A one-shot server: answers the first request with `reply` and hands back what it got.
    fn serve(reply: &'static [u8]) -> Option<(String, mpsc::Receiver<Vec<u8>>)> {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let Ok((mut s, _)) = listener.accept() else { return };
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let mut got = Vec::new();
            let mut buf = [0u8; 8192];
            // read the head, then the declared body
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(buf.get(..n).unwrap_or(&[]));
                if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head =
                        String::from_utf8_lossy(got.get(..end).unwrap_or(&[])).to_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if got.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let _ = s.write_all(reply);
            let _ = tx.send(got);
        });
        Some((format!("http://127.0.0.1:{port}"), rx))
    }

    #[test]
    fn urls_parse_and_https_is_refused() {
        assert_eq!(
            parse_url("http://octopi.local:5000/octoprint/"),
            Ok(HttpUrl { host: "octopi.local".into(), port: 5000, path: "/octoprint".into() })
        );
        assert_eq!(
            parse_url("192.168.1.20"),
            Ok(HttpUrl { host: "192.168.1.20".into(), port: 80, path: String::new() })
        );
        assert_eq!(
            parse_url("http://[::1]:7125").map(|u| (u.host, u.port)),
            Ok(("::1".into(), 7125))
        );
        assert_eq!(parse_url("https://printer"), Err(NetError::Https));
        assert!(parse_url("").is_err());
        assert!(parse_url("http://host:99999").is_err());
        assert!(parse_url("http://a b").is_err());
        assert!(parse_url("ftp://x").is_err());
        assert!(parse_url("http://user@host").is_err());
    }

    #[test]
    fn responses_parse_including_chunked() {
        let r = parse_response(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nokEXTRA");
        assert_eq!(r.map(|r| (r.status, r.body)), Ok((201, b"ok".to_vec())));
        let r = parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n");
        assert_eq!(r.map(|r| r.body), Ok(b"abcde".to_vec()));
        assert!(parse_response(b"garbage").is_err());
        assert!(parse_response(b"HTTP/1.1 abc\r\n\r\n").is_err());
        assert!(
            parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffff\r\nab")
                .is_err()
        );
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_file_name("crown"), "crown.gcode");
        assert_eq!(safe_file_name("../../etc/pass wd.gcode"), "_.._etc_pass wd.gcode");
        assert_eq!(safe_file_name(""), "slicecraft.gcode");
        assert_eq!(safe_file_name("моя"), "___.gcode");
    }

    #[test]
    fn the_key_is_never_shown() {
        let p = NetworkPrinter { api_key: "SECRET123".into(), ..NetworkPrinter::default() };
        assert!(!format!("{p:?}").contains("SECRET123"));
        let e = NetError::Status { status: 401, message: "x".into() };
        assert!(e.to_string().contains("API key"));
    }

    #[test]
    fn octoprint_upload_sends_a_multipart_form_with_the_key() {
        let Some((url, rx)) =
            serve(b"HTTP/1.1 201 CREATED\r\nContent-Length: 13\r\n\r\n{\"done\":true}")
        else {
            return;
        };
        let p = NetworkPrinter { kind: HostKind::OctoPrint, url, api_key: "K3Y".into() };
        let r = upload(&p, "part one", b"G28\nG1 X10\n", true);
        assert!(r.as_ref().is_ok_and(|m| m.contains("started")), "{r:?}");
        let got = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
        let text = String::from_utf8_lossy(&got);
        assert!(text.starts_with("POST /api/files/local HTTP/1.1"));
        assert!(text.contains("X-Api-Key: K3Y"));
        assert!(text.contains("filename=\"part one.gcode\""));
        assert!(text.contains("name=\"print\"\r\n\r\ntrue"));
        assert!(text.contains("G28\nG1 X10\n"));
    }

    #[test]
    fn moonraker_check_and_a_refused_key() {
        let Some((url, _rx)) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 42\r\n\r\n{\"result\": {\"klippy_state\": \"ready\"}}    ") else {
            return;
        };
        let p = NetworkPrinter { kind: HostKind::Moonraker, url, api_key: String::new() };
        let r = check(&p);
        assert!(r.as_ref().is_ok_and(|m| m.contains("ready")), "{r:?}");
        let Some((url, _rx)) = serve(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n") else {
            return;
        };
        let p = NetworkPrinter { kind: HostKind::OctoPrint, url, api_key: "bad".into() };
        assert!(matches!(
            upload(&p, "x", b"G28", false),
            Err(NetError::Status { status: 403, .. })
        ));
    }

    #[test]
    fn a_closed_port_is_an_error_not_a_hang() {
        let port = TcpListener::bind("127.0.0.1:0")
            .ok()
            .and_then(|l| l.local_addr().ok())
            .map_or(1, |a| a.port());
        let p = NetworkPrinter { url: format!("127.0.0.1:{port}"), ..NetworkPrinter::default() };
        assert!(matches!(check(&p), Err(NetError::Connect(_))));
    }
}
