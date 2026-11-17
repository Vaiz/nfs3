use std::io::{self, Read, Write};

#[cfg(feature = "opaque-diagnostics")]
use nfs3_types::xdr_codec::opaque_diagnostics::{Count, Ledger, take};
use nfs3_types::xdr_codec::{Opaque, Pack, Unpack};

// RFC 4506 section 4.10: u32 length, uninterpreted bytes, zero padding.
// These fixtures are intentionally independent of the codec/padding helpers.
#[test]
fn opaque_golden_bytes() {
    let fixtures: &[(&[u8], &[u8])] = &[
        (&[], &[0, 0, 0, 0]),
        (&[0x61], &[0, 0, 0, 1, 0x61, 0, 0, 0]),
        (&[0x61, 0x62], &[0, 0, 0, 2, 0x61, 0x62, 0, 0]),
        (&[0x61, 0x62, 0x63], &[0, 0, 0, 3, 0x61, 0x62, 0x63, 0]),
        (&[0, 0xff, 0x80, 1], &[0, 0, 0, 4, 0, 0xff, 0x80, 1]),
    ];
    for &(payload, wire) in fixtures {
        let mut encoded = Vec::new();
        assert_eq!(
            Opaque::borrowed(payload).pack(&mut encoded).expect("pack"),
            wire.len()
        );
        assert_eq!(encoded, wire);
        let (decoded, read) = Opaque::unpack(&mut &wire[..]).expect("unpack");
        assert_eq!(decoded.as_ref(), payload);
        assert_eq!(read, wire.len());
    }
}

#[test]
fn opaque_truncated_payload() {
    assert!(Opaque::unpack(&mut &[0, 0, 0, 3, 0x61][..]).is_err());
}

// Exercise the codec's existing read_exact/write_all loops, without changing IO.
struct ShortIo<T> {
    inner: T,
    remaining: usize,
    interrupt: bool,
}

impl<T> ShortIo<T> {
    const fn new(inner: T, remaining: usize) -> Self {
        Self {
            inner,
            remaining,
            interrupt: true,
        }
    }
}

impl<T: Read> Read for ShortIo<T> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if std::mem::take(&mut self.interrupt) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let len = out.len().min(self.remaining).min(1);
        let read = self.inner.read(&mut out[..len])?;
        self.remaining -= read;
        Ok(read)
    }
}

impl<T: Write> Write for ShortIo<T> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if std::mem::take(&mut self.interrupt) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let len = input.len().min(self.remaining).min(1);
        let written = self.inner.write(&input[..len])?;
        self.remaining -= written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[test]
fn opaque_short_io_and_every_truncation() {
    let wire = [0, 0, 0, 1, b'a', 0, 0, 0];
    let mut out = ShortIo::new(Vec::new(), wire.len());
    assert_eq!(
        Opaque::borrowed(b"a").pack(&mut out).expect("short pack"),
        8
    );
    assert_eq!(out.inner, wire);
    let (decoded, read) =
        Opaque::unpack(&mut ShortIo::new(wire.as_slice(), wire.len())).expect("short unpack");
    assert_eq!(decoded.as_ref(), b"a");
    assert_eq!(read, 8);
    for boundary in 0..wire.len() {
        let mut out = ShortIo::new(Vec::new(), boundary);
        assert!(Opaque::borrowed(b"a").pack(&mut out).is_err());
        assert_eq!(out.inner, wire[..boundary]);
        assert!(Opaque::unpack(&mut ShortIo::new(wire.as_slice(), boundary)).is_err());
    }
}

#[cfg(feature = "opaque-diagnostics")]
const fn done(bytes: u64) -> Count {
    Count {
        attempts: 1,
        attempted_bytes: bytes,
        completions: 1,
        completed_bytes: bytes,
    }
}

#[cfg(feature = "opaque-diagnostics")]
#[test]
fn opaque_independent_ledger() {
    // Independent ledger: payload 1, padding 3, decoder scratch initialized 4.
    let _ = take();
    let mut wire = Vec::new();
    Opaque::borrowed(b"a").pack(&mut wire).expect("pack");
    let (retained, _) = Opaque::unpack(&mut &wire[..]).expect("unpack");
    assert_eq!(
        take(),
        Ledger {
            encode_payload: done(1),
            encode_padding: done(3),
            decode_payload: done(1),
            decode_padding: done(3),
            payload_zero_fill: done(1),
            padding_zero_fill: done(4),
            lost_updates: 0,
        }
    );
    // Keeping/dropping output is NOT a free-origin or retention event.
    drop(retained);
    assert_eq!(take(), Ledger::default());

    Opaque::borrowed(b"").pack(&mut wire).expect("empty pack");
    Opaque::unpack(&mut &[0, 0, 0, 0][..]).expect("empty unpack");
    assert_eq!(
        take(),
        Ledger {
            encode_payload: done(0),
            encode_padding: done(0),
            decode_payload: done(0),
            payload_zero_fill: done(0),
            ..Ledger::default()
        }
    );

    assert!(Opaque::unpack(&mut &[0, 0, 0, 3, 0x61][..]).is_err());
    assert_eq!(
        take(),
        Ledger {
            decode_payload: Count {
                attempts: 1,
                attempted_bytes: 3,
                ..Count::default()
            },
            payload_zero_fill: done(3),
            ..Ledger::default()
        }
    );
}

#[cfg(feature = "opaque-diagnostics")]
#[test]
fn opaque_failure_ledgers() {
    let wire = [0, 0, 0, 1, b'a', 0, 0, 0];
    for boundary in 0..wire.len() {
        let _ = take();
        let mut out = ShortIo::new(Vec::new(), boundary);
        assert!(Opaque::borrowed(b"a").pack(&mut out).is_err());
        let mut expected = Ledger::default();
        if boundary >= 4 {
            expected.encode_payload = Count {
                attempts: 1,
                attempted_bytes: 1,
                ..Count::default()
            };
        }
        if boundary >= 5 {
            expected.encode_payload = done(1);
            expected.encode_padding = Count {
                attempts: 1,
                attempted_bytes: 3,
                ..Count::default()
            };
        }
        assert_eq!(take(), expected, "encode boundary {boundary}");

        assert!(Opaque::unpack(&mut ShortIo::new(wire.as_slice(), boundary)).is_err());
        let mut expected = Ledger::default();
        if boundary >= 4 {
            expected.payload_zero_fill = done(1);
            expected.decode_payload = Count {
                attempts: 1,
                attempted_bytes: 1,
                ..Count::default()
            };
        }
        if boundary >= 5 {
            expected.decode_payload = done(1);
            expected.padding_zero_fill = done(4);
            expected.decode_padding = Count {
                attempts: 1,
                attempted_bytes: 3,
                ..Count::default()
            };
        }
        assert_eq!(take(), expected, "decode boundary {boundary}");
    }
}
