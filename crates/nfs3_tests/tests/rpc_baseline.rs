use std::io;
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use nfs3_tests::rpc_baseline::{CASES, Case, Tap, Transcript, golden, validate_transcript};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[cfg(feature = "rpc-diagnostics")]
#[test]
fn conservation_rejects_counter_loss_unmatched_io_and_nonquiescence() {
    use nfs3_client::nfs3_types::rpc_diagnostics::{
        Count, Growth, Ledger, Lifecycle, SelectedSocketReads,
    };
    use nfs3_client::nfs3_types::xdr_codec::opaque_diagnostics;
    use nfs3_tests::rpc_baseline::validate_ledger;
    let c = |calls, bytes| Count {
        attempts: calls,
        attempted_bytes: bytes,
        completions: calls,
        completed_bytes: bytes,
    };
    // Independent NULL fixture: one complete record, final boundary EOF.
    let mut ledger = Ledger {
        counts: [
            c(1, 44),
            c(1, 44),
            c(1, 4),
            c(1, 24),
            c(1, 24),
            c(1, 4),
            c(1, 131072),
            c(1, 44),
            Count {
                attempts: 2,
                attempted_bytes: 8,
                completions: 1,
                completed_bytes: 4,
            },
            c(1, 40),
            c(1, 40),
            c(2, 8),
            c(1, 24),
            c(1, 4),
            c(1, 24),
            c(0, 0),
            c(0, 0),
            c(3, 72),
        ],
        selected: SelectedSocketReads {
            ok_calls: 1,
            ok_bytes: 44,
            eof: 1,
            ..Default::default()
        },
        growth: Growth {
            observations: 1,
            capacity_increases: 1,
            old_logical_upper_bound: 0,
        },
        lifecycle: [Lifecycle {
            starts: 1,
            normal_ends: 1,
            drops: 1,
        }; 3],
        marker_eof: 1,
        ..Default::default()
    };
    let opaque = opaque_diagnostics::Ledger {
        encode_payload: c(3, 0),
        encode_padding: c(3, 0),
        decode_payload: c(3, 0),
        payload_zero_fill: c(3, 0),
        ..Default::default()
    };
    let case = Case::parse("null").unwrap();
    validate_ledger(case, &ledger, &opaque).unwrap();
    for index in 0..ledger.counts.len() {
        ledger.counts[index].completions += 1;
        assert!(validate_ledger(case, &ledger, &opaque).is_err());
        ledger.counts[index].completions -= 1;
    }
    let original = ledger;
    for variant in 0..10 {
        ledger = original;
        match variant {
            0 => ledger.loss = 1,
            1 => ledger.selected.loss = 1,
            2 => ledger.selected.eof = 0,
            3 => ledger.selected.other_errors = 1,
            4 => ledger.selected.would_block = 1,
            5 => ledger.selected.ok_bytes = 43,
            6 => ledger.lifecycle[1].normal_ends = 0,
            7 => ledger.lifecycle[2].drops = 0,
            8 => ledger.marker_errors = 1,
            9 => ledger.growth.old_logical_upper_bound = 1,
            _ => unreachable!(),
        }
        assert!(validate_ledger(case, &ledger, &opaque).is_err());
    }
    let mut lost_opaque = opaque;
    lost_opaque.lost_updates = 1;
    assert!(validate_ledger(case, &original, &lost_opaque).is_err());
}

#[test]
fn all_cases_are_isolated_and_watchdog_bounded() {
    for case in CASES {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rpc_baseline"))
            .arg(case)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn isolated case");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait().expect("poll child") {
                assert!(status.success(), "{case}: {status}");
                break;
            }
            if Instant::now() >= deadline {
                child.kill().expect("kill timed out child");
                child.wait().expect("reap timed out child");
                panic!("{case}: external parent watchdog expired; exact claims invalid");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(feature = "rpc-diagnostics")]
#[test]
fn cancelled_task_guard_is_not_a_normal_end() {
    use nfs3_client::nfs3_types::rpc_diagnostics::{self as d, Guard, Lifecycle};
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let _ = d::take();
            let task = tokio::spawn(async {
                let _guard = Guard::start(1);
                std::future::pending::<()>().await;
            });
            tokio::task::yield_now().await;
            assert_eq!(d::snapshot().lifecycle[1].starts, 1);
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
            assert_eq!(
                d::take().lifecycle[1],
                Lifecycle {
                    starts: 1,
                    normal_ends: 0,
                    drops: 1
                }
            );
        });
}

#[test]
fn golden_rejects_mutated_xid_handle_verifier_payload_and_overflow() {
    let generation = [1, 2, 3, 4, 5, 6, 7, 8];
    for name in CASES {
        let case = Case::parse(name).unwrap();
        let (request, reply) = golden(case, [0x12, 0x34, 0x56, 0x78], generation);
        assert_eq!((request.len(), reply.len()), case.lengths());
        let mut transcript = Transcript::new(reply.len().max(request.len()));
        transcript.append(true, &request);
        transcript.append(false, &reply);
        validate_transcript(case, &transcript, generation).unwrap();
        // Full equality above checks every byte; representative mutations cover
        // framing, XID echo, attrs, handles, payload and verifier.
        let mut offsets: Vec<_> = (0..transcript.reply.len().min(228)).step_by(4).collect();
        offsets.extend([transcript.reply.len() / 2, transcript.reply.len() - 1]);
        for index in offsets {
            transcript.reply[index] ^= 1;
            assert!(
                validate_transcript(case, &transcript, generation).is_err(),
                "{name}: {index}"
            );
            transcript.reply[index] ^= 1;
        }

        transcript.overflow = true;
        assert!(validate_transcript(case, &transcript, generation).is_err());
    }
    let mut transcript = Transcript::new(3);
    transcript.append(true, &[1, 2]);
    transcript.append(true, &[3, 4]);
    assert!(transcript.overflow);
    assert_eq!(transcript.request, [1, 2]);
    assert_eq!(transcript.request.capacity(), 3);
}

// Independent successful NULL reply with XID echoed from the request prefix.
// Every underlying call transfers at most three bytes, or returns an error.
struct ShortIo {
    written: Vec<u8>,
    reply: [u8; 28],
    read: usize,
    read_error_after: usize,
    write_error_after: usize,
}

impl ShortIo {
    fn new(read_error_after: usize, write_error_after: usize) -> Self {
        Self {
            written: Vec::with_capacity(44),
            reply: [
                0x80, 0, 0, 24, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0,
            ],
            read: 0,
            read_error_after,
            write_error_after,
        }
    }
}

impl AsyncRead for ShortIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.read >= self.read_error_after {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        let n = 3
            .min(buf.remaining())
            .min(self.read_error_after - self.read)
            .min(self.reply.len() - self.read);
        assert!(
            n > 0,
            "fixture must not trigger the client's zero-progress loop"
        );
        buf.put_slice(&self.reply[self.read..self.read + n]);
        self.read += n;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for ShortIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.written.len() >= self.write_error_after {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        let n = 3
            .min(buf.len())
            .min(self.write_error_after - self.written.len());
        self.written.extend_from_slice(&buf[..n]);
        if self.written.len() >= 8 {
            let xid: [u8; 4] = self.written[4..8].try_into().unwrap();
            self.reply[4..8].copy_from_slice(&xid);
        }
        Poll::Ready(Ok(n))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn short_io_and_error_prefixes_do_not_claim_completed_operations() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            for (read_limit, write_limit, expected_request, expected_reply, success) in [
                (28, 44, 44, 28, true),
                (28, 5, 5, 0, false),
                (2, 44, 44, 2, false),
                (7, 44, 44, 7, false),
            ] {
                let transcript = Arc::new(Mutex::new(Transcript::new(128)));
                let tap = Tap {
                    io: ShortIo::new(read_limit, write_limit),
                    transcript: Arc::clone(&transcript),
                };
                let mut client =
                    nfs3_client::Nfs3Client::new(nfs3_client::tokio::TokioIo::new(tap));
                #[cfg(feature = "rpc-diagnostics")]
                let _ = nfs3_client::nfs3_types::rpc_diagnostics::take();
                assert_eq!(client.null().await.is_ok(), success);
                let transcript = transcript.lock().unwrap();
                assert_eq!(transcript.request.len(), expected_request);
                assert_eq!(transcript.reply.len(), expected_reply);
                assert!(!transcript.overflow);
                if success {
                    validate_transcript(Case::parse("null").unwrap(), &transcript, [0; 8]).unwrap();
                }
                #[cfg(feature = "rpc-diagnostics")]
                {
                    use nfs3_client::nfs3_types::rpc_diagnostics::{self as d, Site};
                    let ledger = d::take();
                    let send = ledger.counts[Site::ClientSend as usize];
                    assert_eq!(send.attempted_bytes, 44);
                    assert_eq!(send.completed_bytes, if write_limit == 44 { 44 } else { 0 });
                    let marker = ledger.counts[Site::ClientMarker as usize];
                    assert_eq!(marker.attempts, u64::from(write_limit == 44));
                    assert_eq!(
                        marker.completions,
                        u64::from(write_limit == 44 && read_limit >= 4)
                    );
                    let body = ledger.counts[Site::ClientBody as usize];
                    assert_eq!(
                        body.attempts,
                        u64::from(write_limit == 44 && read_limit >= 4)
                    );
                    assert_eq!(body.completions, u64::from(success));
                    let copy = ledger.counts[Site::TestCopy as usize];
                    assert_eq!(
                        copy.completed_bytes,
                        (expected_request + expected_reply) as u64
                    );
                    assert_eq!(copy.attempts, copy.completions);
                    assert_eq!(ledger.loss, 0);
                }
            }
        });
}
