//! B0.3 normal-system-allocator diagnostics. Not a timing benchmark.
//! Each invocation measures exactly one connection/request on a current-thread
//! runtime. Callers MUST supply an external process watchdog: the unchanged
//! client's zero-progress loops can starve cooperative timeouts.
//! B0.6 uses separate operation callbacks; B0.3's printed timing remains unmeasured.
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use anyhow::{Result, ensure};
use nfs3_client::Nfs3Client;
use nfs3_client::nfs3_types::nfs3::*;
#[cfg(feature = "rpc-diagnostics")]
use nfs3_client::nfs3_types::{
    rpc_diagnostics::{self as diagnostics, Count, Ledger, Site},
    xdr_codec::opaque_diagnostics,
};
use nfs3_client::tokio::TokioIo;
use nfs3_server::vfs::NfsFileSystem;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::Server;
use crate::perf::{ReadFs, WriteFs};

pub const CASES: [&str; 11] = [
    "null",
    "getattr",
    "lookup",
    "read-4",
    "read-64",
    "read-256",
    "read-1024",
    "write-4",
    "write-64",
    "write-256",
    "write-1024",
];
pub const TRANSCRIPT_BOUND: usize = 1024 * 1024 + 256;

#[derive(Clone, Copy, Debug)]
pub struct Case {
    procedure: u32,
    size: usize,
}

impl Case {
    pub fn parse(name: &str) -> Result<Self> {
        ensure!(CASES.contains(&name), "unknown case");
        let procedure = if name.starts_with("read-") {
            6
        } else if name.starts_with("write-") {
            7
        } else {
            match name {
                "null" => 0,
                "getattr" => 1,
                "lookup" => 3,
                _ => unreachable!(),
            }
        };
        let size = name
            .split_once('-')
            .map_or(Ok(4096), |(_, n)| n.parse::<usize>().map(|n| n * 1024))?;
        Ok(Self { procedure, size })
    }

    pub fn lengths(self) -> (usize, usize) {
        match self.procedure {
            0 => (44, 28),
            1 => (64, 116),
            3 => (72, 228),
            6 => (76, self.size + 132),
            7 => (self.size + 84, 164),
            _ => unreachable!(),
        }
    }
}

/// Preallocated bounded successful-prefix capture, excluded from product memory.
pub struct Transcript {
    pub request: Vec<u8>,
    pub reply: Vec<u8>,
    pub overflow: bool,
    bound: usize,
}

impl Transcript {
    pub fn new(bound: usize) -> Self {
        Self {
            request: Vec::with_capacity(bound),
            reply: Vec::with_capacity(bound),
            overflow: false,
            bound,
        }
    }

    pub fn append(&mut self, request: bool, bytes: &[u8]) {
        let buffer = if request {
            &mut self.request
        } else {
            &mut self.reply
        };
        if bytes.len() > self.bound.saturating_sub(buffer.len()) {
            self.overflow = true;
            return;
        }
        buffer.extend_from_slice(bytes);
        #[cfg(feature = "rpc-diagnostics")]
        diagnostics::completed(Site::TestCopy, bytes.len());
    }
}

/// Client-only tap: observes only successful prefixes after the original poll.
/// The product server's select futures are not wrapped or pinned differently.
pub struct Tap<IO> {
    pub io: IO,
    pub transcript: Arc<Mutex<Transcript>>,
}

impl<IO: AsyncRead + Unpin> AsyncRead for Tap<IO> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.io).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            self.transcript
                .lock()
                .expect("transcript lock")
                .append(false, &buf.filled()[before..]);
        }
        result
    }
}

impl<IO: AsyncWrite + Unpin> AsyncWrite for Tap<IO> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.io).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &result {
            self.transcript
                .lock()
                .expect("transcript lock")
                .append(true, &buf[..*n]);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

fn word(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn wide(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn handle(out: &mut Vec<u8>, generation: [u8; 8], id: u64) {
    word(out, 16);
    out.extend_from_slice(&generation);
    out.extend_from_slice(&id.to_ne_bytes());
}

// Independent hand-authored RFC 1813 fattr3 fields, not Pack or helper attrs.
fn attributes(out: &mut Vec<u8>, directory: bool, size: usize) {
    for value in if directory {
        [2, 0o755, 2, 0, 0]
    } else {
        [1, 0o644, 1, 0, 0]
    } {
        word(out, value);
    }
    let size = if directory { 4096 } else { size as u64 };
    wide(out, size);
    wide(out, size);
    word(out, 0); // rdev major
    word(out, 0); // rdev minor
    wide(out, 1); // fsid
    wide(out, if directory { 1 } else { 2 });
    for _ in 0..6 {
        word(out, 0);
    } // atime, mtime, ctime
}

fn frame(mut bytes: Vec<u8>) -> Vec<u8> {
    let length = u32::try_from(bytes.len() - 4).expect("bounded golden");
    bytes[..4].copy_from_slice(&(0x8000_0000 | length).to_be_bytes());
    bytes
}

/// Full independent wire fixtures. XID comes from the actual request; generation
/// comes from the fixture's root handle before measurement, never from replies.
pub fn golden(case: Case, xid: [u8; 4], generation: [u8; 8]) -> (Vec<u8>, Vec<u8>) {
    let mut request = vec![0; 4];
    request.extend_from_slice(&xid);
    for value in [0, 2, 100_003, 3, case.procedure, 0, 0, 0, 0] {
        word(&mut request, value);
    }
    match case.procedure {
        0 => {}
        1 => handle(&mut request, generation, 2),
        3 => {
            handle(&mut request, generation, 1);
            word(&mut request, 4);
            request.extend_from_slice(b"file");
        }
        6 | 7 => {
            handle(&mut request, generation, 2);
            wide(&mut request, 0);
            word(&mut request, case.size as u32);
            if case.procedure == 7 {
                word(&mut request, 2); // FILE_SYNC
                word(&mut request, case.size as u32);
                request.resize(request.len() + case.size, 0xAA);
            }
        }
        _ => unreachable!(),
    }
    let mut reply = vec![0; 4];
    reply.extend_from_slice(&xid);
    for value in [1, 0, 0, 0, 0] {
        word(&mut reply, value);
    }
    if case.procedure != 0 {
        word(&mut reply, 0);
    } // NFS3_OK
    match case.procedure {
        0 => {}
        1 => attributes(&mut reply, false, case.size),
        3 => {
            handle(&mut reply, generation, 2);
            word(&mut reply, 1);
            attributes(&mut reply, false, case.size);
            word(&mut reply, 1);
            attributes(&mut reply, true, case.size);
        }
        6 => {
            word(&mut reply, 1);
            attributes(&mut reply, false, case.size);
            word(&mut reply, case.size as u32);
            word(&mut reply, 1); // EOF
            word(&mut reply, case.size as u32);
            reply.resize(reply.len() + case.size, 0xAA);
        }
        7 => {
            word(&mut reply, 1); // pre-op attributes
            wide(&mut reply, case.size as u64);
            for _ in 0..4 {
                word(&mut reply, 0);
            } // mtime/ctime
            word(&mut reply, 1);
            attributes(&mut reply, false, case.size);
            word(&mut reply, case.size as u32);
            word(&mut reply, 2);
            reply.extend_from_slice(&generation);
        }
        _ => unreachable!(),
    }
    (frame(request), frame(reply))
}

pub fn validate_transcript(case: Case, transcript: &Transcript, generation: [u8; 8]) -> Result<()> {
    ensure!(!transcript.overflow, "transcript overflow");
    let (request_len, reply_len) = case.lengths();
    ensure!(transcript.request.len() == request_len, "request length");
    ensure!(transcript.reply.len() == reply_len, "reply length");
    let xid = transcript.request[4..8].try_into()?;
    let (request, reply) = golden(case, xid, generation);
    ensure!(
        transcript.request == request,
        "request differs from hand-authored wire"
    );
    ensure!(
        transcript.reply == reply,
        "reply differs from hand-authored wire / XID echo"
    );
    Ok(())
}

#[cfg(feature = "rpc-diagnostics")]
fn count(calls: u64, bytes: usize) -> Count {
    Count {
        attempts: calls,
        attempted_bytes: bytes as u64,
        completions: calls,
        completed_bytes: bytes as u64,
    }
}

#[cfg(feature = "rpc-diagnostics")]
pub fn validate_ledger(
    case: Case,
    ledger: &Ledger,
    opaque: &opaque_diagnostics::Ledger,
) -> Result<()> {
    let (request, reply) = case.lengths();
    ensure!(
        ledger.loss == 0 && ledger.selected.loss == 0 && opaque.lost_updates == 0,
        "counter loss"
    );
    ensure!(
        ledger.selected.eof == 1
            && ledger.selected.would_block == 0
            && ledger.selected.other_errors == 0,
        "selected socket termination"
    );
    ensure!(
        ledger.selected.ok_bytes == request as u64,
        "selected conservation"
    );
    ensure!(
        ledger.selected.ok_calls > 0 && ledger.counts[Site::TestCopy as usize].completions > 0,
        "missing successful calls"
    );
    ensure!(
        ledger.marker_eof == 1 && ledger.marker_errors == 0,
        "boundary EOF"
    );
    ensure!(ledger.bad_write_bytes == 0, "sink pattern");
    for lifecycle in ledger.lifecycle {
        ensure!(
            lifecycle
                == diagnostics::Lifecycle {
                    starts: 1,
                    normal_ends: 1,
                    drops: 1
                },
            "not quiescent: {lifecycle:?}"
        );
    }
    ensure!(
        ledger.growth
            == diagnostics::Growth {
                observations: 1,
                capacity_increases: 1,
                old_logical_upper_bound: 0,
            },
        "single-fragment growth"
    );
    let mut expected = Ledger::default().counts;
    for (site, calls, bytes) in [
        (Site::ClientEncode, 1, request),
        (Site::ClientSend, 1, request),
        (Site::ClientMarker, 1, 4),
        (Site::ClientBody, 1, reply - 4),
        (Site::ClientZero, 1, reply - 4),
        (Site::ClientMarkerZero, 1, 4),
        (Site::ServerScratch, 1, 128 * 1024),
        (Site::Forward, ledger.selected.ok_calls, request),
        (Site::ServerBody, 1, request - 4),
        (Site::ServerZero, 1, request - 4),
        (Site::ServerMarkerZero, 2, 8),
        (Site::ServerEncode, 1, reply - 4),
        (Site::ServerSendMarker, 1, 4),
        (Site::ServerSendBody, 1, reply - 4),
        (
            Site::TestCopy,
            ledger.counts[Site::TestCopy as usize].completions,
            request + reply,
        ),
    ] {
        expected[site as usize] = count(calls, bytes);
    }
    expected[Site::ServerMarker as usize] = Count {
        attempts: 2,
        attempted_bytes: 8,
        completions: 1,
        completed_bytes: 4,
    };
    if case.procedure == 6 {
        expected[Site::ReadCopy as usize] = count(1, case.size);
    }
    if case.procedure == 7 {
        expected[Site::WriteConsumed as usize] = count(1, case.size);
    }
    // The unmatched final four-byte attempt is valid ONLY together with full
    // independent request/reply conservation and normal local guard termination.
    ensure!(ledger.counts == expected, "RPC site counters: {ledger:?}");
    let (calls, bytes) = match case.procedure {
        0 => (3, 0),
        1 => (4, 16),
        3 => (6, 36),
        6 | 7 => (5, case.size + 16),
        _ => unreachable!(),
    };
    let expected = opaque_diagnostics::Ledger {
        encode_payload: count(calls, bytes),
        encode_padding: count(calls, 0),
        decode_payload: count(calls, bytes),
        payload_zero_fill: count(calls, bytes),
        ..opaque_diagnostics::Ledger::default()
    };
    ensure!(
        *opaque == expected,
        "opaque counters (including empty auth/handles/names): {opaque:?}"
    );
    Ok(())
}

enum Output {
    Null,
    Getattr(GETATTR3resok),
    Lookup(LOOKUP3resok),
    Read(READ3resok<'static>),
    Write(WRITE3resok),
}

fn success<T, E: std::fmt::Debug>(result: Nfs3Result<T, E>) -> Result<T> {
    match result {
        Nfs3Result::Ok(value) => Ok(value),
        Nfs3Result::Err(error) => anyhow::bail!("NFS error: {error:?}"),
    }
}

pub fn run(name: &str) -> Result<()> {
    run_observed(name, &mut NoopObserver)
}

/// Synchronous harness boundaries; the normal B0.3 instantiation is a no-op.
pub trait Observer {
    const CAPTURE: bool = false;
    const PRINT: bool = true;
    fn setup(&mut self) {}
    fn operation_start(&mut self) {}
    fn operation_end(&mut self) {}
    fn retained(&mut self, _output: Option<OwnedAllocation>) {}
    fn dropped(&mut self) {}
    fn teardown(&mut self) {}
}

pub struct NoopObserver;
impl Observer for NoopObserver {}

/// Numeric observation only: no payload copy or ownership transfer.
#[derive(Clone, Copy, Debug)]
pub struct OwnedAllocation {
    pub address: usize,
    pub length: usize,
    pub capacity: usize,
}

pub fn run_observed<O: Observer>(name: &str, observer: &mut O) -> Result<()> {
    let case = Case::parse(name)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = if case.procedure == 7 {
        runtime.block_on(run_inner(
            name,
            case,
            WriteFs::new(case.size as u64),
            observer,
        ))
    } else {
        runtime.block_on(run_inner(name, case, ReadFs::new(case.size), observer))
    };
    drop(runtime);
    observer.teardown();
    result
}

async fn run_inner<FS: NfsFileSystem + 'static, O: Observer>(
    name: &str,
    case: Case,
    fs: FS,
    observer: &mut O,
) -> Result<()> {
    let transcript = Arc::new(Mutex::new(Transcript::new(TRANSCRIPT_BOUND)));
    let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
    let server = Server::new(server_io, fs)?;
    let root = server.root_dir();
    ensure!(
        root.data.len() == 16 && root.data[8..] == 1_u64.to_ne_bytes(),
        "root fixture"
    );
    let generation: [u8; 8] = root.data[..8].try_into()?;
    let mut file_bytes = Vec::from(generation);
    file_bytes.extend_from_slice(&2_u64.to_ne_bytes());
    let file = nfs_fh3 {
        data: file_bytes.into(),
    };
    let input = vec![0xAA; case.size];
    let mut client = Nfs3Client::new(TokioIo::new(Tap {
        io: client_io,
        transcript: Arc::clone(&transcript),
    }));
    #[cfg(feature = "rpc-diagnostics")]
    {
        let _ = diagnostics::take();
        let _ = opaque_diagnostics::take();
    }
    observer.setup();
    let server_task = tokio::spawn(server.run());
    observer.operation_start();
    let output = match case.procedure {
        0 => {
            client.null().await?;
            Output::Null
        }
        1 => Output::Getattr(success(
            client.getattr(&GETATTR3args { object: file }).await?,
        )?),
        3 => Output::Lookup(success(
            client
                .lookup(&LOOKUP3args {
                    what: diropargs3 {
                        dir: root,
                        name: b"file".as_slice().into(),
                    },
                })
                .await?,
        )?),
        6 => Output::Read(success(
            client
                .read(&READ3args {
                    file,
                    offset: 0,
                    count: case.size as u32,
                })
                .await?,
        )?),
        7 => Output::Write(success(
            client
                .write(&WRITE3args {
                    file,
                    offset: 0,
                    count: case.size as u32,
                    stable: stable_how::FILE_SYNC,
                    data: input.as_slice().into(),
                })
                .await?,
        )?),
        _ => unreachable!(),
    };
    observer.operation_end();
    // Server send completion must be observed before disconnect. Feature-off
    // yields only harness scheduling; the complete captured reply still proves IO.
    #[cfg(feature = "rpc-diagnostics")]
    {
        let mut sent = false;
        for _ in 0..1024 {
            if diagnostics::snapshot().counts[Site::ServerSendBody as usize].completions == 1 {
                sent = true;
                break;
            }
            tokio::task::yield_now().await;
        }
        ensure!(sent, "server send did not complete");
    }
    #[cfg(not(feature = "rpc-diagnostics"))]
    tokio::task::yield_now().await;
    drop(client);
    server_task.await??;
    // Reader's final marker read must fail normally, not be cancelled by runtime
    // drop. No reset here: that read may already have been pending for a while.
    #[cfg(feature = "rpc-diagnostics")]
    {
        let mut quiescent = false;
        for _ in 0..1024 {
            let ledger = diagnostics::snapshot();
            if ledger
                .lifecycle
                .iter()
                .all(|l| l.starts == 1 && l.normal_ends == 1 && l.drops == 1)
            {
                quiescent = true;
                break;
            }
            tokio::task::yield_now().await;
        }
        ensure!(
            quiescent,
            "reader/handler not normally quiescent inside block_on"
        );
    }
    #[cfg(not(feature = "rpc-diagnostics"))]
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    #[cfg(feature = "rpc-diagnostics")]
    let (ledger, opaque) = (diagnostics::take(), opaque_diagnostics::take());
    let transcript = transcript.lock().expect("transcript lock");
    validate_transcript(case, &transcript, generation)?;
    #[cfg(feature = "rpc-diagnostics")]
    validate_ledger(case, &ledger, &opaque)?;
    let (retained_logical, retained_capacity) = match &output {
        Output::Null => (0, 0),
        Output::Getattr(result) => {
            ensure!(
                result.obj_attributes.size == case.size as u64,
                "getattr size"
            );
            (0, 0)
        }
        Output::Lookup(result) => {
            let mut expected = Vec::from(generation);
            expected.extend_from_slice(&2_u64.to_ne_bytes());
            ensure!(
                result.object.data.as_ref() == expected,
                "lookup file handle"
            );
            (
                result.object.data.len(),
                owned_capacity(&result.object.data.0),
            )
        }
        Output::Read(result) => {
            ensure!(
                result.count as usize == case.size && result.eof,
                "read count/EOF"
            );
            ensure!(result.data.iter().all(|&b| b == 0xAA), "read pattern");
            (result.data.len(), owned_capacity(&result.data.0))
        }
        Output::Write(result) => {
            ensure!(
                result.count as usize == case.size && result.committed == stable_how::FILE_SYNC,
                "write count/stability"
            );
            ensure!(result.verf.0 == generation, "write verifier");
            (0, 0)
        }
    };
    std::hint::black_box(&output);
    if O::CAPTURE {
        let owned = match &output {
            Output::Lookup(result) => Some(observe_owned(&result.object.data.0)?),
            Output::Read(result) => Some(observe_owned(&result.data.0)?),
            _ => None,
        };
        observer.retained(owned);
    }
    if O::PRINT {
        println!(
            "B0.3 case={name}; hooks={}; allocator=system; runtime=current_thread; requests=1; \
             connections=1; harness=passed",
            cfg!(feature = "rpc-diagnostics")
        );
        println!(
            "wire_request={}; wire_reply={}; golden=passed; transcript_overflow={}; \
             checkpoint=output_retained; output_logical={retained_logical}; \
             output_capacity_observed={retained_capacity}",
            transcript.request.len(),
            transcript.reply.len(),
            transcript.overflow
        );
        #[cfg(feature = "rpc-diagnostics")]
        {
            println!(
                "selected={:?}; forward={:?}; scratch_zero={:?}; explicit_read_copy={:?}; \
                 write_consumed={:?}; test_capture_copy={:?}; growth={:?}; marker_eof={}; \
                 lifecycle={:?}",
                ledger.selected,
                ledger.counts[Site::Forward as usize],
                ledger.counts[Site::ServerScratch as usize],
                ledger.counts[Site::ReadCopy as usize],
                ledger.counts[Site::WriteConsumed as usize],
                ledger.counts[Site::TestCopy as usize],
                ledger.growth,
                ledger.marker_eof,
                ledger.lifecycle
            );
            println!("rpc={ledger:?}");
            println!("opaque={opaque:?}");
        }
    }
    drop(output);
    observer.dropped();
    if O::PRINT {
        println!(
            "checkpoint=output_dropped; exact_lifetime_accounting=unsupported; \
             timing=not_measured; physical_copy_total=unknown; \
             handle_conversion_copies=uninstrumented; verifier_copies=uninstrumented; \
             internal_and_harness_duplex_copies=uninstrumented; \
             header_field_codecs=uninstrumented; cancelled_select_reads=uninstrumented; \
             initiated_socket_reads=uninstrumented; capacity_growth=observation_not_realloc; \
             old_logical_relocation=upper_bound_not_actual; process_memory=unknown; \
             instrumentation_overhead=unknown"
        );
    }
    Ok(())
}

#[allow(
    clippy::ptr_arg,
    reason = "borrowed outputs and owned capacity must remain distinguishable"
)]
fn observe_owned(bytes: &std::borrow::Cow<'_, [u8]>) -> Result<OwnedAllocation> {
    let std::borrow::Cow::Owned(bytes) = bytes else {
        anyhow::bail!("owned LOOKUP/READ output required");
    };
    ensure!(
        !bytes.is_empty() && bytes.capacity() != 0,
        "empty owned output"
    );
    Ok(OwnedAllocation {
        address: bytes.as_ptr() as usize,
        length: bytes.len(),
        capacity: bytes.capacity(),
    })
}

#[allow(
    clippy::ptr_arg,
    reason = "a slice hides the retained owned Vec capacity"
)]
fn owned_capacity(bytes: &std::borrow::Cow<'_, [u8]>) -> usize {
    match bytes {
        std::borrow::Cow::Owned(vec) => vec.capacity(),
        std::borrow::Cow::Borrowed(_) => 0,
    }
}
