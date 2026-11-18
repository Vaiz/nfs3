use std::fmt::Debug;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail, ensure};
use nfs3_client::Nfs3Client;
use nfs3_client::nfs3_types::nfs3::*;
use nfs3_client::nfs3_types::xdr_codec::Pack;
use nfs3_client::tokio::TokioIo;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::fixture::{Fixture, pattern};
use super::{Chunks, WIRE_BOUND, fragments, reassemble};
use crate::Server;
use crate::rpc_baseline::{Case, Tap, Transcript, golden};

const CAPACITY: usize = 1024;
// Permit large odd outer reads in raw WRITE cases. The scheduling proof uses
// the deliberately small CAPACITY, never this transport.
const RAW_CAPACITY: usize = 16 * 1024;
const PAGE_BOUND: u32 = 1024;
const PAGE_LIMIT: usize = 101;
const ONE: &[usize] = &[1];
// First split the marker even for small READ requests; subsequent caps include
// large odd chunks for the 1 MiB WRITE lane.
const ODD: &[usize] = &[3, 4093, 17, 8191];
const FULL: &[usize] = &[WIRE_BOUND];

pub fn cases() -> Vec<String> {
    let mut names = Vec::new();
    for split in 0..=40 {
        for chunk in ["one", "odd"] {
            names.push(format!("null-{split}-{chunk}"));
        }
    }
    for op in ["read", "write"] {
        for size in [4, 1024] {
            for record in ["single", "fragmented"] {
                for chunk in ["full", "chunked"] {
                    names.push(format!("{op}-{size}-{record}-{chunk}"));
                }
            }
        }
    }
    for op in ["readdir", "plus"] {
        for count in [0, 3, 100] {
            names.push(format!("{op}-{count}"));
        }
    }
    names.extend(
        [
            "zero-verifier",
            "slow-request",
            "slow-reader",
            "tcp-read-nodelay",
            "tcp-read-nagle",
            "tcp-write-nodelay",
            "tcp-write-nagle",
            "empty-record",
            "partial-marker",
            "partial-body",
            "fragmented-reply",
        ]
        .map(str::to_owned),
    );
    names
}

#[derive(Default)]
struct Totals {
    request: usize,
    reply: usize,
    request_markers: usize,
    requests: usize,
    connections: usize,
}

impl Totals {
    fn add(&mut self, request: usize, reply: usize, markers: usize) {
        self.request += request;
        self.reply += reply;
        self.request_markers += markers;
        self.requests += 1;
    }
}

enum Retained {
    None,
    Directory {
        pages: Vec<Page>,
        count: usize,
        generation: [u8; 8],
    },
}

enum Page {
    Plain(READDIR3resok<'static>),
    Plus(READDIRPLUS3resok<'static>),
}

fn success<T, E: Debug>(value: Nfs3Result<T, E>) -> Result<T> {
    match value {
        Nfs3Result::Ok(value) => Ok(value),
        Nfs3Result::Err(error) => bail!("NFS error: {error:?}"),
    }
}

fn generation(root: &nfs_fh3) -> Result<[u8; 8]> {
    ensure!(
        root.data.len() == 16 && root.data[8..] == 1u64.to_ne_bytes(),
        "root fixture"
    );
    Ok(root.data[..8].try_into()?)
}

fn file(generation: [u8; 8], id: u64) -> nfs_fh3 {
    let mut data = generation.to_vec();
    data.extend_from_slice(&id.to_ne_bytes());
    nfs_fh3 { data: data.into() }
}

fn patterned_golden(name: &str, xid: [u8; 4], generation: [u8; 8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let (mut request, mut reply) = golden(Case::parse(name)?, xid, generation);
    if name.starts_with("read-") {
        for (i, b) in reply[132..].iter_mut().enumerate() {
            *b = pattern(i);
        }
    } else if name.starts_with("write-") {
        for (i, b) in request[84..].iter_mut().enumerate() {
            *b = pattern(i);
        }
    }
    Ok((request, reply))
}

async fn reply(io: &mut (impl AsyncRead + Unpin)) -> Result<Vec<u8>> {
    let mut marker = [0; 4];
    io.read_exact(&mut marker).await?;
    let word = u32::from_be_bytes(marker);
    ensure!(word & 0x8000_0000 != 0, "server reply not single fragment");
    let len = (word & 0x7fff_ffff) as usize;
    ensure!(len <= WIRE_BOUND - 4, "reply capture bound");
    let mut wire = vec![0; len + 4];
    wire[..4].copy_from_slice(&marker);
    io.read_exact(&mut wire[4..]).await?;
    Ok(wire)
}

async fn exchange(
    io: &mut (impl AsyncRead + AsyncWrite + Unpin),
    request: &[u8],
) -> Result<Vec<u8>> {
    // The server runs independently. One request outstanding; keep the write
    // half open until its full reply has been read.
    io.write_all(request).await?;
    reply(io).await
}

/// Only inspect completed body-send bytes at the explicitly requested stalled
/// checkpoint. This is not a successful final ledger nor a counter reset.
fn completed_send_bytes() -> Option<u64> {
    #[cfg(feature = "rpc-diagnostics")]
    {
        use nfs3_client::nfs3_types::rpc_diagnostics::{self as d, Site};
        Some(d::snapshot().counts[Site::ServerSendBody as usize].completed_bytes)
    }
    #[cfg(not(feature = "rpc-diagnostics"))]
    {
        None
    }
}

async fn sent_at_least(bytes: usize) -> Result<()> {
    // Bounded scheduling steps, not elapsed-time/fairness assertions.
    for _ in 0..4096 {
        if completed_send_bytes().is_none_or(|n| n >= bytes as u64) {
            tokio::task::yield_now().await;
            return Ok(());
        }
        tokio::task::yield_now().await;
    }
    bail!("send completion not observed in bounded harness schedule")
}

fn final_ledger(totals: &Totals, valid: bool) -> Result<()> {
    #[cfg(feature = "rpc-diagnostics")]
    {
        use nfs3_client::nfs3_types::rpc_diagnostics::{self as d, Site};
        use nfs3_client::nfs3_types::xdr_codec::opaque_diagnostics;
        let l = d::snapshot();
        if valid {
            ensure!(l.loss == 0 && l.selected.loss == 0, "counter loss");
            ensure!(
                l.selected.ok_bytes == totals.request as u64,
                "selected conservation"
            );
            ensure!(
                l.counts[Site::Forward as usize].completed_bytes == totals.request as u64,
                "forward conservation"
            );
            ensure!(
                l.counts[Site::ServerBody as usize].completed_bytes
                    == (totals.request - totals.request_markers) as u64,
                "record body conservation"
            );
            ensure!(
                l.counts[Site::ServerMarker as usize].completed_bytes
                    == totals.request_markers as u64,
                "marker conservation"
            );
            ensure!(
                l.counts[Site::ServerSendBody as usize].completed_bytes
                    == (totals.reply - 4 * totals.requests) as u64,
                "reply body conservation"
            );
            ensure!(
                l.counts[Site::ServerSendMarker as usize].completed_bytes
                    == (4 * totals.requests) as u64,
                "reply marker conservation"
            );
            ensure!(
                l.selected.other_errors == 0 && l.selected.would_block == 0,
                "socket error"
            );
            ensure!(
                l.selected.eof == totals.connections as u64
                    && l.marker_eof == totals.connections as u64
                    && l.marker_errors == 0,
                "normal boundary EOF"
            );
            ensure!(
                l.growth.observations == (totals.request_markers / 4) as u64
                    && l.growth.capacity_increases <= l.growth.observations
                    && l.growth.old_logical_upper_bound
                        <= (totals.request * (totals.request_markers / 4)) as u64,
                "bounded growth observations"
            );
            for (index, life) in l.lifecycle.iter().enumerate() {
                let expected = if index == 2 {
                    totals.requests
                } else {
                    totals.connections
                };
                ensure!(
                    life.starts == expected as u64
                        && life.normal_ends == life.starts
                        && life.drops == life.starts,
                    "nonquiescent: {life:?}"
                );
            }
            // Reset the opaque ledger only after all instrumented work has
            // demonstrably ended. Error outcomes make no normal-ledger claim.
            let opaque = opaque_diagnostics::take();
            ensure!(opaque.lost_updates == 0, "opaque counter loss");
            println!("opaque={opaque:?}");
        }
        println!(
            "ledger_valid_success={valid}; ledger={l:?}; \
             growth=logical_relocation_upper_not_actual_copy; errors_opaque=not_reset"
        );
    }
    #[cfg(not(feature = "rpc-diagnostics"))]
    let _ = (totals, valid);
    Ok(())
}

pub fn run(name: &str) -> Result<()> {
    ensure!(cases().iter().any(|case| case == name), "unknown case");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let retained = runtime.block_on(async {
        #[cfg(feature = "rpc-diagnostics")]
        {
            let _ = nfs3_client::nfs3_types::rpc_diagnostics::take();
            let _ = nfs3_client::nfs3_types::xdr_codec::opaque_diagnostics::take();
        }
        let mut totals = Totals::default();
        let (retained, outcome) =
            if name.starts_with("null-") || name.starts_with("read-") || name.starts_with("write-")
            {
                raw(name, &mut totals).await?;
                (Retained::None, "VALID_PASS")
            } else {
                other(name, &mut totals).await?
            };
        final_ledger(&totals, outcome == "VALID_PASS")?;
        println!(
            "case={name}; outcome={outcome}; hooks={}; allocator=system; runtime=current_thread; \
             attribution=aggregate; connections={}; requests={}; request_wire={}; \
             request_markers={}; request_body={}; reply_wire={}; \
             small_duplex_capacity={CAPACITY}; wire_bound={WIRE_BOUND}; page_bound={PAGE_BOUND}; \
             page_limit={PAGE_LIMIT}; timing=not_measured",
            cfg!(feature = "rpc-diagnostics"),
            totals.connections,
            totals.requests,
            totals.request,
            totals.request_markers,
            totals.request.saturating_sub(totals.request_markers),
            totals.reply
        );
        Ok::<_, anyhow::Error>(retained)
    })?;
    drop(runtime);
    if let Retained::Directory {
        pages,
        count,
        generation,
    } = retained
    {
        validate_pages(&pages, count, generation)?;
        println!(
            "retained_after_client_server_runtime_drop=validated; entries={count}; pages={}; \
             backing_memory=unknown",
            pages.len()
        );
    }
    Ok(())
}

async fn raw(name: &str, totals: &mut Totals) -> Result<()> {
    let parts: Vec<_> = name.split('-').collect();
    let null = parts[0] == "null";
    let size = if null {
        4096
    } else {
        parts[1].parse::<usize>()? * 1024
    };
    let schedule = if null {
        if parts[2] == "one" { ONE } else { ODD }
    } else if parts[3] == "full" {
        FULL
    } else if size == 4096 {
        ONE
    } else {
        ODD
    };
    let fs = Fixture::new(size, 0, false);
    let written = Arc::clone(&fs.written);
    let (server_io, client_io) = tokio::io::duplex(RAW_CAPACITY);
    let server = Server::new(Chunks::new(server_io, schedule), fs)?;
    let gen_ = generation(&server.root_dir())?;
    let base = if null {
        "null".to_owned()
    } else {
        format!("{}-{}", parts[0], parts[1])
    };
    let (request, expected) = patterned_golden(&base, [0x12, 0x34, 0x56, 0x78], gen_)?;
    let cuts = if null {
        vec![parts[1].parse()?]
    } else if parts[2] == "fragmented" {
        vec![1, 13, (request.len() / 2) | 1, request.len() - 5]
    } else {
        vec![]
    };
    let wire = fragments(&request[4..], &cuts)?;
    let transcript = Arc::new(Mutex::new(Transcript::new(WIRE_BOUND)));
    let mut io = Tap {
        io: client_io,
        transcript: Arc::clone(&transcript),
    };
    let task = tokio::spawn(server.run());
    let actual = exchange(&mut io, &wire).await?;
    ensure!(actual == expected, "full hand-built reply mismatch");
    let (body, markers) = {
        let t = transcript.lock().expect("transcript");
        ensure!(
            !t.overflow && t.request == wire && t.reply == expected,
            "captured wire mismatch"
        );
        reassemble(&t.request)?
    };
    ensure!(
        body == request[4..] && wire.len() == body.len() + markers,
        "request model/marker conservation"
    );
    sent_at_least(expected.len() - 4).await?;
    drop(io);
    task.await??;
    ensure!(
        written.load(Ordering::SeqCst) == if parts[0] == "write" { size } else { 0 },
        "pattern sink bytes"
    );
    totals.connections = 1;
    totals.add(wire.len(), expected.len(), markers);
    println!(
        "lane=raw_server_path_not_real_client_encode; fragments={}; \
         outer_read_schedule={schedule:?}; raw_duplex_capacity={RAW_CAPACITY}; \
         internal_segmentation=uncontrolled; extra_marker_bytes={}",
        cuts.len() + 1,
        markers - 4
    );
    Ok(())
}

fn word(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_be_bytes());
}
fn wide(out: &mut Vec<u8>, n: u64) {
    out.extend_from_slice(&n.to_be_bytes());
}
fn frame(out: &mut [u8]) {
    let n = (out.len() - 4) as u32 | 0x8000_0000;
    out[..4].copy_from_slice(&n.to_be_bytes());
}

// Independent RFC fields and fixture constants, not server pagination formulas.
fn attributes(out: &mut Vec<u8>, id: u64, directory: bool, nonzero: bool) {
    for n in if directory {
        [2, 0o755, 2, 0, 0]
    } else {
        [1, 0o644, 1, 0, 0]
    } {
        word(out, n);
    }
    let size = if directory { 4096 } else { 1024 };
    wide(out, size);
    wide(out, size);
    word(out, 0);
    word(out, 0);
    wide(out, 1);
    wide(out, id);
    for n in [
        0,
        0,
        if nonzero { 17 } else { 0 },
        if nonzero { 29 } else { 0 },
        0,
        0,
    ] {
        word(out, n);
    }
}

fn directory_request(
    plus: bool,
    xid: [u8; 4],
    gen_: [u8; 8],
    cookie: u64,
    verf: cookieverf3,
) -> Vec<u8> {
    let (mut out, _) = golden(Case::parse("null").expect("constant"), xid, gen_);
    out[24..28].copy_from_slice(&(if plus { 17u32 } else { 16u32 }).to_be_bytes());
    word(&mut out, 16);
    out.extend_from_slice(&gen_);
    out.extend_from_slice(&1u64.to_ne_bytes());
    wide(&mut out, cookie);
    out.extend_from_slice(&verf.0);
    word(&mut out, PAGE_BOUND);
    if plus {
        word(&mut out, PAGE_BOUND);
    }
    frame(&mut out);
    out
}

fn page_model(
    plus: bool,
    first: usize,
    len: usize,
    eof: bool,
    gen_: [u8; 8],
    verf: cookieverf3,
    nonzero_mtime: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    word(&mut out, 1);
    attributes(&mut out, 1, true, nonzero_mtime);
    out.extend_from_slice(&verf.0);
    for i in first..first + len {
        word(&mut out, 1); // linked-list presence
        wide(&mut out, (i + 10) as u64);
        let name = format!("entry_{i:05}");
        word(&mut out, name.len() as u32);
        out.extend_from_slice(name.as_bytes());
        while out.len() % 4 != 0 {
            out.push(0);
        }
        wide(&mut out, (i + 1) as u64);
        if plus {
            word(&mut out, 1);
            attributes(&mut out, (i + 10) as u64, false, false);
            word(&mut out, 1);
            word(&mut out, 16);
            out.extend_from_slice(&gen_);
            out.extend_from_slice(&((i + 10) as u64).to_ne_bytes());
        }
    }
    word(&mut out, 0);
    word(&mut out, u32::from(eof));
    out
}

impl Page {
    fn info(&self) -> (bool, usize, bool, cookieverf3, u64) {
        match self {
            Self::Plain(p) => (
                false,
                p.reply.entries.0.len(),
                p.reply.eof,
                p.cookieverf,
                p.reply.entries.0.last().map_or(0, |e| e.cookie),
            ),
            Self::Plus(p) => (
                true,
                p.reply.entries.0.len(),
                p.reply.eof,
                p.cookieverf,
                p.reply.entries.0.last().map_or(0, |e| e.cookie),
            ),
        }
    }
    fn encoded(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        match self {
            Self::Plain(p) => {
                p.pack(&mut bytes)?;
            }
            Self::Plus(p) => {
                p.pack(&mut bytes)?;
            }
        }
        Ok(bytes)
    }
}

fn validate_pages(pages: &[Page], count: usize, gen_: [u8; 8]) -> Result<()> {
    ensure!(
        !pages.is_empty() && pages.len() <= PAGE_LIMIT,
        "page count bound"
    );
    ensure!(
        count != 100 || pages.len() >= 2,
        "100 entries require pagination"
    );
    let mut first = 0;
    let verifier = pages[0].info().3;
    ensure!(
        verifier != cookieverf3::default(),
        "valid fixture has nonzero verifier"
    );
    for (index, page) in pages.iter().enumerate() {
        let (plus, len, eof, verf, _) = page.info();
        ensure!(eof == (index + 1 == pages.len()), "only final EOF");
        ensure!(eof || len > 0, "empty nonterminal page");
        ensure!(
            first + len <= count && verf == verifier,
            "entry bound/stable verifier"
        );
        let bytes = page.encoded()?;
        ensure!(
            bytes.len() <= PAGE_BOUND as usize,
            "encoded resok exceeds count/maxcount"
        );
        ensure!(
            bytes == page_model(plus, first, len, eof, gen_, verifier, true),
            "independent entries/attrs/handles/padding model"
        );
        first += len;
    }
    ensure!(first == count, "missing or duplicated entries");
    Ok(())
}

async fn directory(name: &str, totals: &mut Totals) -> Result<(Retained, &'static str)> {
    let plus = name.starts_with("plus-");
    let count = name.split_once('-').expect("case").1.parse::<usize>()?;
    let (server_io, client_io) = tokio::io::duplex(CAPACITY);
    let server = Server::new(server_io, Fixture::new(1024, count, true))?;
    let root = server.root_dir();
    let gen_ = generation(&root)?;
    let transcript = Arc::new(Mutex::new(Transcript::new(WIRE_BOUND)));
    let mut client = Nfs3Client::new(TokioIo::new(Tap {
        io: client_io,
        transcript: Arc::clone(&transcript),
    }));
    let task = tokio::spawn(server.run());
    let mut pages = Vec::new();
    let mut cookie = 0;
    let mut verf = cookieverf3::default();
    for _ in 0..PAGE_LIMIT {
        let (request_start, reply_start) = {
            let t = transcript.lock().expect("transcript");
            (t.request.len(), t.reply.len())
        };
        let page = if plus {
            Page::Plus(success(
                client
                    .readdirplus(&READDIRPLUS3args {
                        dir: root.clone(),
                        cookie,
                        cookieverf: verf,
                        dircount: PAGE_BOUND,
                        maxcount: PAGE_BOUND,
                    })
                    .await?,
            )?)
        } else {
            Page::Plain(success(
                client
                    .readdir(&READDIR3args {
                        dir: root.clone(),
                        cookie,
                        cookieverf: verf,
                        count: PAGE_BOUND,
                    })
                    .await?,
            )?)
        };
        let (_, len, eof, returned, next) = page.info();
        ensure!(len <= count && (eof || len > 0), "page progress");
        let encoded = page.encoded()?;
        {
            let t = transcript.lock().expect("transcript");
            ensure!(!t.overflow, "directory capture overflow");
            let request = &t.request[request_start..];
            ensure!(request.len() >= 8, "request prefix");
            let xid = request[4..8].try_into()?;
            ensure!(
                request == directory_request(plus, xid, gen_, cookie, verf),
                "independent directory request"
            );
            let (_, mut expected) = golden(Case::parse("null")?, xid, gen_);
            word(&mut expected, 0);
            expected.extend_from_slice(&encoded);
            frame(&mut expected);
            ensure!(
                t.reply[reply_start..] == expected,
                "full single-fragment directory reply"
            );
            totals.add(request.len(), expected.len(), 4);
        }
        pages.push(page);
        cookie = next;
        verf = returned; // echo the opaque server verifier, never synthesize resume tokens
        if eof {
            break;
        }
    }
    sent_at_least(totals.reply - 4 * totals.requests).await?;
    drop(client);
    task.await??;
    totals.connections = 1;
    validate_pages(&pages, count, gen_)?;
    Ok((
        Retained::Directory {
            pages,
            count,
            generation: gen_,
        },
        "VALID_PASS",
    ))
}

async fn other(name: &str, totals: &mut Totals) -> Result<(Retained, &'static str)> {
    if name.starts_with("readdir-") || name.starts_with("plus-") {
        directory(name, totals).await
    } else if name.starts_with("slow-") {
        progress(name == "slow-reader", totals).await?;
        Ok((Retained::None, "VALID_PASS"))
    } else if name.starts_with("tcp-") {
        tcp(name, totals).await?;
        Ok((Retained::None, "VALID_PASS"))
    } else if name == "zero-verifier" {
        zero_verifier(totals).await?;
        Ok((Retained::None, "KNOWN_PRODUCT_DEFECT"))
    } else if name == "fragmented-reply" {
        unsupported(totals).await?;
        Ok((Retained::None, "UNSUPPORTED"))
    } else {
        malformed(name, totals).await?;
        Ok((Retained::None, "EXPECTED_MALFORMED"))
    }
}

async fn progress(slow_reader: bool, totals: &mut Totals) -> Result<()> {
    let (slow_server_io, slow_client_io) = tokio::io::duplex(CAPACITY);
    let slow_server = Server::new(slow_server_io, Fixture::new(4096, 0, false))?;
    let slow_gen = generation(&slow_server.root_dir())?;
    let base = if slow_reader { "read-4" } else { "null" };
    let (slow_request, slow_expected) = patterned_golden(base, [1, 2, 3, 4], slow_gen)?;
    let slow_capture = Arc::new(Mutex::new(Transcript::new(WIRE_BOUND)));
    let mut slow = Tap {
        io: slow_client_io,
        transcript: Arc::clone(&slow_capture),
    };
    let slow_task = tokio::spawn(slow_server.run());
    let mut marker = [0; 4];
    if slow_reader {
        slow.write_all(&slow_request).await?;
        slow.read_exact(&mut marker).await?;
        ensure!(marker == slow_expected[..4], "slow reply marker");
        ensure!(
            slow_expected.len() - 4 > CAPACITY + 4,
            "capacity proof requires body > C + bytes read"
        );
    } else {
        // A complete marker and seven bytes of body cannot be a complete RPC.
        slow.write_all(&slow_request[..11]).await?;
    }

    // Healthy connection is independently driven. Never await the slow sender
    // or a blocked writer before the healthy reply has been fully validated.
    let (healthy_server_io, healthy_client_io) = tokio::io::duplex(CAPACITY);
    let healthy_server = Server::new(healthy_server_io, Fixture::new(4096, 0, false))?;
    let gen_ = generation(&healthy_server.root_dir())?;
    let (healthy_request, healthy_expected) = patterned_golden("null", [5, 6, 7, 8], gen_)?;
    let healthy_capture = Arc::new(Mutex::new(Transcript::new(WIRE_BOUND)));
    let mut healthy = Tap {
        io: healthy_client_io,
        transcript: Arc::clone(&healthy_capture),
    };
    let healthy_task = tokio::spawn(healthy_server.run());
    ensure!(
        exchange(&mut healthy, &healthy_request).await? == healthy_expected,
        "healthy full body"
    );
    sent_at_least(healthy_expected.len() - 4).await?;
    if let Some(bytes) = completed_send_bytes() {
        ensure!(
            bytes == (healthy_expected.len() - 4) as u64,
            "before release only healthy body may complete"
        );
    }
    println!(
        "healthy_complete_before_release=true; slow_state={}; slow_bytes_read={}; C={CAPACITY}; \
         slow_reply_body={}; delayed_release_steps=1; hook_completed_body_before_release={:?}",
        if slow_reader {
            "unread_reply"
        } else {
            "partial_request"
        },
        if slow_reader { 4 } else { 0 },
        slow_expected.len() - 4,
        completed_send_bytes()
    );
    let slow_actual = if slow_reader {
        let mut wire = vec![0; slow_expected.len()];
        wire[..4].copy_from_slice(&marker);
        slow.read_exact(&mut wire[4..]).await?;
        wire
    } else {
        slow.write_all(&slow_request[11..]).await?;
        reply(&mut slow).await?
    };
    ensure!(slow_actual == slow_expected, "released slow full reply");
    for (capture, request, expected) in [
        (&slow_capture, &slow_request, &slow_expected),
        (&healthy_capture, &healthy_request, &healthy_expected),
    ] {
        let t = capture.lock().expect("transcript");
        ensure!(
            !t.overflow && t.request == *request && t.reply == *expected,
            "two-connection wire"
        );
        ensure!(
            reassemble(&t.request)? == (request[4..].to_vec(), 4),
            "two-connection record model"
        );
        totals.add(request.len(), expected.len(), 4);
    }
    sent_at_least(slow_expected.len() + healthy_expected.len() - 8).await?;
    drop(slow);
    drop(healthy);
    slow_task.await??;
    healthy_task.await??;
    totals.connections = 2;
    Ok(())
}

async fn tcp(name: &str, totals: &mut Totals) -> Result<()> {
    let nodelay = name.ends_with("nodelay");
    let write = name.starts_with("tcp-write");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    ensure!(
        address.ip().is_loopback() && address.port() != 0,
        "ephemeral loopback"
    );
    // Connect/accept exactly once; no NFSTcpListener (which forces NODELAY).
    let client_io = tokio::net::TcpStream::connect(address).await?;
    let (server_io, _) = listener.accept().await?;
    drop(listener);
    client_io.set_nodelay(nodelay)?;
    server_io.set_nodelay(nodelay)?;
    ensure!(
        client_io.nodelay()? == nodelay && server_io.nodelay()? == nodelay,
        "both-end nodelay readback before wrappers"
    );
    let fs = Fixture::new(4096, 0, false);
    let written = Arc::clone(&fs.written);
    let server = Server::new(server_io, fs)?;
    let gen_ = generation(&server.root_dir())?;
    let transcript = Arc::new(Mutex::new(Transcript::new(WIRE_BOUND)));
    let mut client = Nfs3Client::new(TokioIo::new(Tap {
        io: client_io,
        transcript: Arc::clone(&transcript),
    }));
    let task = tokio::spawn(server.run());
    if write {
        let data: Vec<_> = (0..4096).map(pattern).collect();
        let result = success(
            client
                .write(&WRITE3args {
                    file: file(gen_, 2),
                    offset: 0,
                    count: 4096,
                    stable: stable_how::FILE_SYNC,
                    data: data.as_slice().into(),
                })
                .await?,
        )?;
        ensure!(
            result.count == 4096 && result.committed == stable_how::FILE_SYNC,
            "TCP write result"
        );
    } else {
        let result = success(
            client
                .read(&READ3args {
                    file: file(gen_, 2),
                    offset: 0,
                    count: 4096,
                })
                .await?,
        )?;
        ensure!(
            result.count == 4096 && result.eof && result.data.len() == 4096,
            "TCP read result"
        );
        ensure!(
            result
                .data
                .iter()
                .enumerate()
                .all(|(i, b)| *b == pattern(i)),
            "TCP read pattern"
        );
    }
    {
        let t = transcript.lock().expect("transcript");
        ensure!(!t.overflow && t.request.len() >= 8, "TCP capture");
        let (request, expected) = patterned_golden(
            if write { "write-4" } else { "read-4" },
            t.request[4..8].try_into()?,
            gen_,
        )?;
        ensure!(
            t.request == request && t.reply == expected,
            "full independent TCP real-client wire"
        );
        totals.add(request.len(), expected.len(), 4);
    }
    sent_at_least(totals.reply - 4).await?;
    drop(client);
    task.await??;
    ensure!(
        written.load(Ordering::SeqCst) == if write { 4096 } else { 0 },
        "TCP sink"
    );
    totals.connections = 1;
    println!(
        "lane=real_client_process_socket_tcp; accepted_connections=1; nodelay_client={nodelay}; \
         nodelay_server={nodelay}; nodelay_readback=true; kernel_backpressure=not_claimed"
    );
    Ok(())
}

async fn malformed(name: &str, totals: &mut Totals) -> Result<()> {
    let wire = match name {
        "empty-record" => fragments(&[], &[0])?,
        "partial-marker" => vec![0x80, 0],
        "partial-body" => {
            let mut wire = (0x8000_0000u32 | 8192).to_be_bytes().to_vec();
            let (request, _) = golden(Case::parse("null")?, [1, 2, 3, 4], [0; 8]);
            wire.extend_from_slice(&request[4..17]);
            wire
        }
        _ => bail!("unknown malformed case"),
    };
    ensure!(
        reassemble(&wire).is_err(),
        "independent invalid/incomplete record model"
    );
    let (server_io, mut client_io) = tokio::io::duplex(CAPACITY);
    let server = Server::new(server_io, Fixture::new(4096, 0, false))?;
    let task = tokio::spawn(server.run());
    client_io.write_all(&wire).await?;
    client_io.shutdown().await?;
    let mut output = Vec::new();
    (&mut client_io)
        .take(WIRE_BOUND as u64)
        .read_to_end(&mut output)
        .await?;
    let result = task.await?;
    ensure!(
        output.is_empty(),
        "malformed request unexpectedly got a reply"
    );
    drop(client_io);
    // No successful conservation/quiescence claim on these error paths.
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    #[cfg(feature = "rpc-diagnostics")]
    {
        use nfs3_client::nfs3_types::rpc_diagnostics::{self as d, Site};
        let l = d::snapshot();
        ensure!(
            l.loss == 0 && l.selected.loss == 0,
            "error-path counter loss"
        );
        ensure!(
            l.selected.ok_bytes == wire.len() as u64
                && l.counts[Site::Forward as usize].completed_bytes == wire.len() as u64,
            "incomplete/invalid supplied prefix must still be forwarded"
        );
        ensure!(
            l.counts[Site::ServerSendBody as usize].completed_bytes == 0,
            "error path must not complete a reply"
        );
        for life in l.lifecycle {
            ensure!(
                life.starts == life.drops,
                "error-path cleanup still pending: {life:?}"
            );
        }
        println!(
            "error_prefix_forwarded={}; successful_reply_body=0; \
             error_cleanup=all_started_guards_dropped; normal_success_ledger=not_claimed",
            l.counts[Site::Forward as usize].completed_bytes
        );
    }
    totals.connections = 1;
    totals.request = wire.len();
    totals.request_markers = if name == "empty-record" {
        8
    } else {
        wire.len().min(4)
    };
    println!(
        "record_model={}; reply=none; advertised_body_max=8192; supplied_wire={}; \
         process_socket_ok={}; forwarded_partial_is_not_success=true",
        if name == "empty-record" {
            "all_empty_invalid"
        } else {
            "incomplete"
        },
        wire.len(),
        result.is_ok()
    );
    Ok(())
}

async fn unsupported(totals: &mut Totals) -> Result<()> {
    let (mut fake, client_io) = tokio::io::duplex(CAPACITY);
    let task = tokio::spawn(async move {
        let mut request = [0; 44];
        fake.read_exact(&mut request).await?;
        let (expected, reply) = golden(Case::parse("null")?, request[4..8].try_into()?, [0; 8]);
        ensure!(
            request == expected.as_slice(),
            "fake peer independent real-client request"
        );
        let wire = fragments(&reply[4..], &[7])?;
        ensure!(
            reassemble(&wire)?.0 == reply[4..],
            "fake peer valid fragmented reply model"
        );
        fake.write_all(&wire).await?;
        Ok::<_, anyhow::Error>(wire.len())
    });
    let mut client = Nfs3Client::new(TokioIo::new(client_io));
    let error = client
        .null()
        .await
        .expect_err("client must reject nonfinal reply marker");
    ensure!(
        matches!(error, nfs3_client::RpcError::FragmentedReply),
        "wrong rejection: {error:?}"
    );
    ensure!(
        !error.is_connection_reusable(),
        "unread connection cannot be reused"
    );
    drop(client); // unread connection discarded, never drained or reused
    let bytes = task.await??;
    totals.connections = 1;
    totals.add(44, bytes, 4);
    println!(
        "lane=bounded_fake_peer_not_released_peer; error=RpcError::FragmentedReply; \
         connection=discarded_unread; fake_reply_wire={bytes}; speedup=not_claimed"
    );
    Ok(())
}

async fn zero_verifier(totals: &mut Totals) -> Result<()> {
    let (server_io, client_io) = tokio::io::duplex(CAPACITY);
    // Existing ListFs defaults deliberately untouched.
    let server = Server::new(server_io, crate::perf::ListFs::new(100))?;
    let root = server.root_dir();
    let gen_ = generation(&root)?;
    let task = tokio::spawn(server.run());
    let transcript = Arc::new(Mutex::new(Transcript::new(WIRE_BOUND)));
    let mut client = Nfs3Client::new(TokioIo::new(Tap {
        io: client_io,
        transcript: Arc::clone(&transcript),
    }));
    let first = success(
        client
            .readdir(&READDIR3args {
                dir: root.clone(),
                cookie: 0,
                cookieverf: cookieverf3::default(),
                count: PAGE_BOUND,
            })
            .await?,
    )?;
    ensure!(
        first.cookieverf == cookieverf3::default()
            && !first.reply.eof
            && !first.reply.entries.is_empty(),
        "zero-verifier fixture must paginate"
    );
    let mut encoded = Vec::new();
    first.pack(&mut encoded)?;
    ensure!(
        encoded
            == page_model(
                false,
                0,
                first.reply.entries.0.len(),
                false,
                gen_,
                first.cookieverf,
                false
            ),
        "known-defect first page independent model"
    );
    let cookie = first.reply.entries.0.last().expect("nonempty").cookie;
    let (request_start, reply_start) = {
        let t = transcript.lock().expect("transcript");
        let request = &t.request;
        let xid = request[4..8].try_into()?;
        ensure!(
            *request == directory_request(false, xid, gen_, 0, cookieverf3::default()),
            "known-defect first request"
        );
        let (_, mut expected) = golden(Case::parse("null")?, xid, gen_);
        word(&mut expected, 0);
        expected.extend_from_slice(&encoded);
        frame(&mut expected);
        ensure!(t.reply == expected, "known-defect first full reply");
        totals.add(request.len(), expected.len(), 4);
        (request.len(), expected.len())
    };
    let second = client
        .readdir(&READDIR3args {
            dir: root,
            cookie,
            cookieverf: first.cookieverf,
            count: PAGE_BOUND,
        })
        .await?;
    match second {
        Nfs3Result::Err((nfsstat3::NFS3ERR_BAD_COOKIE, _)) => {}
        other => bail!("known defect outcome changed, review required: {other:?}"),
    }
    {
        let t = transcript.lock().expect("transcript");
        ensure!(!t.overflow, "known-defect capture");
        let request = &t.request[request_start..];
        let xid = request[4..8].try_into()?;
        ensure!(
            request == directory_request(false, xid, gen_, cookie, first.cookieverf),
            "known-defect resumed request echoed verifier"
        );
        let (_, mut expected) = golden(Case::parse("null")?, xid, gen_);
        word(&mut expected, 10003); // RFC NFS3ERR_BAD_COOKIE
        word(&mut expected, 0); // absent optional failure attributes
        frame(&mut expected);
        ensure!(
            t.reply[reply_start..] == expected,
            "known-defect full BAD_COOKIE reply"
        );
        totals.add(request.len(), expected.len(), 4);
    }
    sent_at_least(totals.reply - 8).await?;
    drop(client);
    task.await??;
    totals.connections = 1;
    println!(
        "valid_continuation_rejected=true; zero_mtime=true; returned_verifier=0000000000000000; \
         echoed_verifier=true; resumed_cookie={cookie}; observed=NFS3ERR_BAD_COOKIE; \
         classification=KNOWN_PRODUCT_DEFECT"
    );
    Ok(())
}
