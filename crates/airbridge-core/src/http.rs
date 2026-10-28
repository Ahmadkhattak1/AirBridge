use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};

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
    if headers
        .get("transfer-encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
    {
        let mut out = Vec::new();
        loop {
            let mut size = String::new();
            reader.read_line(&mut size)?;
            let size = usize::from_str_radix(size.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| Error::Http("invalid chunk size".into()))?;
            if size == 0 {
                let mut tail = String::new();
                reader.read_line(&mut tail)?;
                break;
            }
            if out.len().saturating_add(size) > limit {
                return Err(Error::Http("request body too large".into()));
            }
            let start = out.len();
            out.resize(start + size, 0);
            for (i, part) in out[start..].chunks_mut(64 * 1024).enumerate() {
                reader.read_exact(part)?;
                progress(start + i * 64 * 1024 + part.len(), None);
            }
            let mut crlf = [0; 2];
            reader.read_exact(&mut crlf)?;
            if crlf != *b"\r\n" {
                return Err(Error::Http("invalid chunk terminator".into()));
            }
        }
        return Ok(out);
    }
    let size = headers
        .get("content-length")
        .map(|v| v.parse::<usize>())
        .transpose()
        .map_err(|_| Error::Http("invalid content length".into()))?
        .unwrap_or(0);
    if size > limit {
        return Err(Error::Http("request body too large".into()));
    }
    let mut out = vec![0; size];
    for (i, part) in out.chunks_mut(64 * 1024).enumerate() {
        reader.read_exact(part)?;
        progress(i * 64 * 1024 + part.len(), Some(size));
    }
    Ok(out)
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
    let mut first = String::new();
    reader.read_line(&mut first)?;
    if first.is_empty() {
        return Err(Error::Http("connection closed".into()));
    }
    let (method, path) = request_line(&first)?;
    let headers = headers(reader)?;
    if headers
        .get("expect")
        .is_some_and(|value| value.eq_ignore_ascii_case("100-continue"))
    {
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        reader.get_mut().flush()?;
    }
    let body = if path == "/Upload" {
        body_progress(reader, &headers, limit, &mut progress)?
    } else {
        body(reader, &headers, limit.min(8 * 1024 * 1024))?
    };
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
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
    write!(out, "{} {} HTTP/1.1\r\n", request.method, request.path)?;
    for (k, v) in &request.headers {
        write!(out, "{k}: {v}\r\n")?;
    }
    if !request.headers.contains_key("content-length")
        && !request.headers.contains_key("transfer-encoding")
    {
        write!(out, "Content-Length: {}\r\n", request.body.len())?;
    }
    write!(out, "\r\n")?;
    out.write_all(&request.body)?;
    out.flush()?;
    Ok(())
}

pub fn write_response<W: Write>(mut out: W, response: &HttpResponse) -> Result<()> {
    write!(out, "HTTP/1.1 {} {}\r\n", response.status, response.reason)?;
    for (k, v) in &response.headers {
        write!(out, "{k}: {v}\r\n")?;
    }
    if !response.headers.contains_key("content-length") {
        write!(out, "Content-Length: {}\r\n", response.body.len())?;
    }
    write!(out, "\r\n")?;
    out.write_all(&response.body)?;
    out.flush()?;
    Ok(())
}
