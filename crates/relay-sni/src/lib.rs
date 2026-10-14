//! Reads the SNI host name from the first TLS record of a connection.
//!
//! The hosted relay routes by this name and forwards the bytes untouched; it
//! never holds a key that could decrypt them.

/// Why no name was returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SniError {
    /// Not enough bytes yet; read more and call again.
    Incomplete,
    /// Not a TLS ClientHello, or malformed.
    NotClientHello,
    /// A valid ClientHello without a host name.
    NoServerName,
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, SniError> {
        let b = *self.buf.get(self.pos).ok_or(SniError::NotClientHello)?;
        self.pos += 1;
        Ok(b)
    }
    fn u16(&mut self) -> Result<usize, SniError> {
        Ok(((self.u8()? as usize) << 8) | self.u8()? as usize)
    }
    fn u24(&mut self) -> Result<usize, SniError> {
        Ok((self.u16()? << 8) | self.u8()? as usize)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], SniError> {
        let end = self.pos.checked_add(n).ok_or(SniError::NotClientHello)?;
        let s = self
            .buf
            .get(self.pos..end)
            .ok_or(SniError::NotClientHello)?;
        self.pos = end;
        Ok(s)
    }
}

/// Largest ClientHello record the relay waits for.
pub const MAX_RECORD: usize = 16 * 1024 + 5;

/// Returns the lowercase host name from a ClientHello at the start of `buf`.
pub fn server_name(buf: &[u8]) -> Result<String, SniError> {
    if buf.len() < 5 {
        return Err(SniError::Incomplete);
    }
    if buf[0] != 0x16 || buf[1] != 0x03 {
        return Err(SniError::NotClientHello);
    }
    let record_len = ((buf[3] as usize) << 8) | buf[4] as usize;
    if record_len + 5 > MAX_RECORD {
        return Err(SniError::NotClientHello);
    }
    let Some(record) = buf.get(5..5 + record_len) else {
        return Err(SniError::Incomplete);
    };
    let mut r = Reader {
        buf: record,
        pos: 0,
    };
    if r.u8()? != 0x01 {
        return Err(SniError::NotClientHello);
    }
    // A ClientHello split across records is legal but is not sent by the
    // clients the relay serves; treat it as malformed rather than buffering.
    let hello_len = r.u24()?;
    let hello = r.take(hello_len)?;
    let mut h = Reader { buf: hello, pos: 0 };
    h.take(2)?; // legacy_version
    h.take(32)?; // random
    let n = h.u8()? as usize;
    h.take(n)?; // session id
    let n = h.u16()?;
    h.take(n)?; // cipher suites
    let n = h.u8()? as usize;
    h.take(n)?; // compression methods
    if h.pos == hello.len() {
        return Err(SniError::NoServerName);
    }
    let ext_len = h.u16()?;
    let mut e = Reader {
        buf: h.take(ext_len)?,
        pos: 0,
    };
    while e.pos < e.buf.len() {
        let kind = e.u16()?;
        let len = e.u16()?;
        let data = e.take(len)?;
        if kind != 0x0000 {
            continue;
        }
        let mut s = Reader { buf: data, pos: 0 };
        let list_len = s.u16()?;
        let mut list = Reader {
            buf: s.take(list_len)?,
            pos: 0,
        };
        while list.pos < list.buf.len() {
            let name_type = list.u8()?;
            let n = list.u16()?;
            let name = list.take(n)?;
            if name_type == 0 {
                let host = std::str::from_utf8(name).map_err(|_| SniError::NotClientHello)?;
                if host.is_empty()
                    || !host
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
                {
                    return Err(SniError::NotClientHello);
                }
                return Ok(host.to_ascii_lowercase());
            }
        }
    }
    Err(SniError::NoServerName)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_hello(host: Option<&str>) -> Vec<u8> {
        let mut ext = Vec::new();
        if let Some(host) = host {
            let mut sni = Vec::new();
            sni.push(0u8);
            sni.extend((host.len() as u16).to_be_bytes());
            sni.extend(host.as_bytes());
            let mut body = (sni.len() as u16).to_be_bytes().to_vec();
            body.extend(sni);
            ext.extend(0u16.to_be_bytes());
            ext.extend((body.len() as u16).to_be_bytes());
            ext.extend(body);
        }
        // An unrelated extension (supported_versions) before or after SNI.
        ext.extend([0x00, 0x2b, 0x00, 0x03, 0x02, 0x03, 0x04]);

        let mut hello = vec![0x03, 0x03];
        hello.extend([0u8; 32]);
        hello.push(0); // session id
        hello.extend([0x00, 0x02, 0x13, 0x01]); // one cipher suite
        hello.extend([0x01, 0x00]); // null compression
        hello.extend((ext.len() as u16).to_be_bytes());
        hello.extend(ext);

        let mut hs = vec![0x01];
        hs.extend(&(hello.len() as u32).to_be_bytes()[1..]);
        hs.extend(hello);
        let mut rec = vec![0x16, 0x03, 0x01];
        rec.extend((hs.len() as u16).to_be_bytes());
        rec.extend(hs);
        rec
    }

    #[test]
    fn reads_the_host_name() {
        let rec = client_hello(Some("Abc123.d.relay.studiocat.app"));
        assert_eq!(server_name(&rec).unwrap(), "abc123.d.relay.studiocat.app");
    }

    #[test]
    fn asks_for_more_bytes_when_truncated() {
        let rec = client_hello(Some("x.d.relay.studiocat.app"));
        for cut in [0, 4, 5, rec.len() - 1] {
            assert_eq!(
                server_name(&rec[..cut]),
                Err(SniError::Incomplete),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn rejects_non_tls_and_missing_names() {
        assert_eq!(
            server_name(b"GET / HTTP/1.1\r\n"),
            Err(SniError::NotClientHello)
        );
        assert_eq!(
            server_name(&client_hello(None)),
            Err(SniError::NoServerName)
        );
        assert_eq!(
            server_name(&client_hello(Some("bad host"))),
            Err(SniError::NotClientHello)
        );
    }

    #[test]
    fn never_panics_on_corrupted_input() {
        let rec = client_hello(Some("x.d.relay.studiocat.app"));
        for i in 5..rec.len() {
            for b in [0x00, 0xff] {
                let mut c = rec.clone();
                c[i] = b;
                let _ = server_name(&c);
            }
        }
    }
}
