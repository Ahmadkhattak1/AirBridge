use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::ops::ControlFlow;

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub reason: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

fn headers<R: BufRead>(reader: &mut R) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(Error::Http("unexpected end of headers".into()));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(Error::Http("malformed header".into()));
        };
        out.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        if out.len() > 128 {
            return Err(Error::Http("too many headers".into()));
        }
    }
    Ok(out)
}

fn body<R: BufRead>(
    reader: &mut R,
    headers: &BTreeMap<String, String>,
    limit: usize,
) -> Result<Vec<u8>> {
    body_progress(reader, headers, limit, &mut |_, _| {})
}

fn body_progress<R: BufRead>(
    reader: &mut R,
    headers: &BTreeMap<String, String>,
    limit: usize,
    progress: &mut impl FnMut(usize, Option<usize>),
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    copy_body(reader, headers, limit, progress, &mut |bytes| {
        out.extend_from_slice(bytes);
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(out)
}

fn copy_body<R: BufRead>(
    reader: &mut R,
    fields: &BTreeMap<String, String>,
    limit: usize,
    progress: &mut impl FnMut(usize, Option<usize>),
    sink: &mut impl FnMut(&[u8]) -> Result<ControlFlow<()>>,
) -> Result<()> {
    let chunked = fields
        .get("transfer-encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"));
    let total = if chunked {
        None
    } else {
        Some(
            fields
                .get("content-length")
                .map(|v| v.parse::<usize>())
                .transpose()
                .map_err(|_| Error::Http("invalid content length".into()))?
                .unwrap_or(0),
        )
    };
    let mut received = 0usize;
    loop {
        let mut remaining = if let Some(length) = total {
            length
        } else {
            let mut size = String::new();
            let n = reader.take(1026).read_line(&mut size)?;
            if n == 0 {
                return Err(Error::Http(format!(
                    "connection ended before the final HTTP chunk after {received} bytes"
                )));
            }
            if !size.ends_with("\r\n") || size.len() > 1024 {
                return Err(Error::Http(format!(
                    "incomplete or oversized chunk header after {received} bytes"
                )));
            }
            let digits = size.trim().split(';').next().unwrap_or("").trim_end();
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(Error::Http(format!(
                    "invalid chunk size after {received} bytes (header {:?})",
                    size.chars().take(64).collect::<String>()
                )));
            }
            let size = usize::from_str_radix(digits, 16)
                .map_err(|_| Error::Http("chunk size exceeds supported range".into()))?;
            if size == 0 {
                headers(reader)?; // Consume all trailers before the next request.
                return Ok(());
            }
            size
        };
        if remaining > limit - received {
            return Err(Error::Http("request body too large".into()));
        }
        while remaining > 0 {
            let bytes = reader.fill_buf()?;
            if bytes.is_empty() {
                return Err(Error::Http(
                    "connection ended during the request body".into(),
                ));
            }
            let n = remaining.min(bytes.len()).min(64 * 1024);
            let action = sink(&bytes[..n])?;
            reader.consume(n);
            received += n;
            remaining -= n;
            progress(received, total);
            if action.is_break() {
                return Ok(());
            }
        }
        if !chunked {
            return Ok(());
        }
        let mut crlf = [0; 2];
        reader.read_exact(&mut crlf)?;
        if crlf != *b"\r\n" {
            return Err(Error::Http("invalid chunk terminator".into()));
        }
    }
}

pub fn read_request<R: Read>(input: R, limit: usize) -> Result<HttpRequest> {
    let mut reader = BufReader::new(input);
    read_request_buffered(&mut reader, limit)
}

pub fn read_request_buffered<R: BufRead>(reader: &mut R, limit: usize) -> Result<HttpRequest> {
    read_request_parts(reader, limit, || Ok(()))
}

/// Reads a request from a persistent connection and honours HTTP's
/// `Expect: 100-continue` handshake before reading a potentially large body.
/// Apple clients use this for AirDrop uploads.
pub fn read_request_buffered_continue<S: Read + Write>(
    reader: &mut BufReader<S>,
    limit: usize,
) -> Result<HttpRequest> {
    read_request_buffered_progress(reader, limit, |_, _| {})
}

pub fn read_request_buffered_progress<S: Read + Write>(
    reader: &mut BufReader<S>,
    limit: usize,
    mut progress: impl FnMut(usize, Option<usize>),
) -> Result<HttpRequest> {
    let mut request = read_request_head(reader)?;
    let limit = if request.path == "/Upload" {
        limit
    } else {
        limit.min(8 * 1024 * 1024)
    };
    let mut body = Vec::new();
    read_request_body_to(
        reader,
        &request,
        limit,
        |bytes, total| {
            if request.path == "/Upload" {
                progress(bytes, total);
            }
        },
        |bytes| {
            body.extend_from_slice(bytes);
            Ok(())
        },
    )?;
    request.body = body;
    Ok(request)
}

/// Read headers separately so the receiver can authorize an upload before
/// acknowledging 100-continue or reading any file bytes.
pub fn read_request_head<R: BufRead>(reader: &mut R) -> Result<HttpRequest> {
    let mut first = String::new();
    reader.read_line(&mut first)?;
    if first.is_empty() {
        return Err(Error::Http("connection closed".into()));
    }
    let (method, path) = request_line(&first)?;
    let headers = headers(reader)?;
    Ok(HttpRequest {
        method,
        path,
        headers,
        body: Vec::new(),
    })
}

/// Copy a framed body in bounded pieces, including arbitrarily large HTTP chunks.
pub fn read_request_body_to<S: Read + Write>(
    reader: &mut BufReader<S>,
    request: &HttpRequest,
    limit: usize,
    progress: impl FnMut(usize, Option<usize>),
    mut sink: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    read_request_body_until(reader, request, limit, progress, |bytes| {
        sink(bytes)?;
        Ok(ControlFlow::Continue(()))
    })
}

/// Read a framed body until the sink signals completion. On early completion
/// the caller must close the connection after responding.
pub fn read_request_body_until<S: Read + Write>(
    reader: &mut BufReader<S>,
    request: &HttpRequest,
    limit: usize,
    mut progress: impl FnMut(usize, Option<usize>),
    mut sink: impl FnMut(&[u8]) -> Result<ControlFlow<()>>,
) -> Result<()> {
    if request
        .headers
        .get("expect")
        .is_some_and(|value| value.eq_ignore_ascii_case("100-continue"))
    {
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        reader.get_mut().flush()?;
    }
    copy_body(reader, &request.headers, limit, &mut progress, &mut sink)
}

fn read_request_parts<R: BufRead, F: FnOnce() -> Result<()>>(
    reader: &mut R,
    limit: usize,
    before_body: F,
) -> Result<HttpRequest> {
    let mut first = String::new();
    reader.read_line(&mut first)?;
    if first.is_empty() {
        return Err(Error::Http("connection closed".into()));
    }
    let (method, path) = request_line(&first)?;
    let headers = headers(reader)?;
    before_body()?;
    let body = body(reader, &headers, limit)?;
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn request_line(first: &str) -> Result<(String, String)> {
    let mut fields = first.split_whitespace();
    let method = fields
        .next()
        .ok_or_else(|| Error::Http("missing method".into()))?
        .to_owned();
    let path = fields
        .next()
        .ok_or_else(|| Error::Http("missing path".into()))?
        .to_owned();
    if fields.next() != Some("HTTP/1.1") {
        return Err(Error::Http("HTTP/1.1 required".into()));
    }
    Ok((method, path))
}

pub fn read_response<R: Read>(input: R, limit: usize) -> Result<HttpResponse> {
    let mut reader = BufReader::new(input);
    read_response_buffered(&mut reader, limit)
}

pub fn read_response_buffered<R: BufRead>(reader: &mut R, limit: usize) -> Result<HttpResponse> {
    let mut first = String::new();
    if reader.read_line(&mut first)? == 0 {
        return Err(Error::Http(
            "peer closed before sending an HTTP response".into(),
        ));
    }
    let mut fields = first.trim().splitn(3, ' ');
    if fields.next() != Some("HTTP/1.1") {
        return Err(Error::Http(format!(
            "unexpected response status line: {:?}",
            first.chars().take(100).collect::<String>()
        )));
    }
    let status = fields
        .next()
        .ok_or_else(|| Error::Http("missing status".into()))?
        .parse()
        .map_err(|_| Error::Http("invalid status".into()))?;
    let reason = fields.next().unwrap_or_default().to_owned();
    let headers = headers(reader)?;
    let body = body(reader, &headers, limit)?;
    Ok(HttpResponse {
        status,
        reason,
        headers,
        body,
    })
}

pub fn write_request<W: Write>(mut out: W, request: &HttpRequest) -> Result<()> {
    let mut head = Vec::new();
    write!(head, "{} {} HTTP/1.1\r\n", request.method, request.path)?;
    for (k, v) in &request.headers {
        write!(head, "{k}: {v}\r\n")?;
    }
    if !request.headers.contains_key("content-length")
        && !request.headers.contains_key("transfer-encoding")
    {
        write!(head, "Content-Length: {}\r\n", request.body.len())?;
    }
    write!(head, "\r\n")?;
    out.write_all(&head)?;
    out.write_all(&request.body)?;
    out.flush()?;
    Ok(())
}

pub fn write_response<W: Write>(mut out: W, response: &HttpResponse) -> Result<()> {
    let mut head = Vec::new();
    write!(head, "HTTP/1.1 {} {}\r\n", response.status, response.reason)?;
    for (k, v) in &response.headers {
        write!(head, "{k}: {v}\r\n")?;
    }
    if !response.headers.contains_key("content-length") {
        write!(head, "Content-Length: {}\r\n", response.body.len())?;
    }
    write!(head, "\r\n")?;
    out.write_all(&head)?;
    out.write_all(&response.body)?;
    out.flush()?;
    Ok(())
}
