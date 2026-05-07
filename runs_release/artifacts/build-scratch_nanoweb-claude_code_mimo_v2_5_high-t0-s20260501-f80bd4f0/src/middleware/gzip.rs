use crate::http::types::{Request, Response};
use crate::middleware::Middleware;

pub struct GzipCompression;

impl Middleware for GzipCompression {
    fn before(&self, _req: &mut Request) -> Option<Response> {
        None
    }

    fn after(&self, req: &Request, mut resp: Response) -> Response {
        let accept = req.header("accept-encoding").unwrap_or("");
        if accept.contains("gzip") && resp.body.len() > 128 {
            if let Some(compressed) = gzip_compress(&resp.body) {
                resp.body = compressed;
                resp.headers.insert("content-encoding", "gzip");
                resp.headers
                    .insert("content-length", resp.body.len().to_string());
            }
        }
        resp
    }

    fn name(&self) -> &str {
        "gzip"
    }
}

fn gzip_compress(data: &[u8]) -> Option<Vec<u8>> {
    // Minimal gzip implementation: store as stored blocks
    // GZIP header
    let mut out = Vec::with_capacity(data.len() + 18);
    out.extend_from_slice(&[0x1f, 0x8b]); // magic
    out.push(0x08); // deflate method
    out.push(0x00); // flags
    out.extend_from_slice(&[0, 0, 0, 0]); // mtime
    out.push(0x00); // xfl
    out.push(0xff); // OS

    // Deflate stored (no compression) block
    let len = data.len();
    let nlen = !len & 0xffff;
    out.push(0x00); // BFINAL=0, BTYPE=00 (stored)
    out.push((len & 0xff) as u8);
    out.push(((len >> 8) & 0xff) as u8);
    out.push((nlen & 0xff) as u8);
    out.push(((nlen >> 8) & 0xff) as u8);
    out.extend_from_slice(data);

    // Adler-32 checksum
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &b in data {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    let adler = (s2 << 16) | s1;
    out.extend_from_slice(&adler.to_be_bytes());

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_middleware_name() {
        assert_eq!(GzipCompression.name(), "gzip");
    }

    #[test]
    fn gzip_compress_produces_valid_header() {
        let data = b"hello world, this is a test of gzip compression";
        let compressed = gzip_compress(data).unwrap();
        assert_eq!(compressed[0], 0x1f);
        assert_eq!(compressed[1], 0x8b);
    }
}
