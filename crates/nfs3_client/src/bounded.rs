//! Opt-in finite RPC transport. Legacy entry points are unchanged.

use nfs3_types::async_io::{FramingError, RecordLimits, send};
use nfs3_types::rpc::{
    RPC_VERSION_2, accept_stat_data, call_body, msg_body, opaque_auth, rejected_reply, reply_body,
    rpc_msg,
};
use nfs3_types::xdr_codec::{
    BackingBudget, BackingReservation, Decode, DecodeError, DecodeLimits, Decoder, Encode,
    EncodeError, EncodeLimits, Encoder, Payload, PayloadError, RecordBuilder,
};

use crate::io::{AsyncRead, AsyncWrite};

/// Explicit finite generic RPC policy. No product defaults are supplied.
#[derive(Debug, Copy, Clone)]
pub struct RpcLimits {
    pub block_bytes: usize,
    pub max_record_bytes: usize,
    pub max_fragments: usize,
    /// Per-client outstanding reservations plus live backing capacity.
    pub backing_bytes: usize,
    /// Cumulative builder descriptor/control reservation work per record.
    pub max_builder_reservation_bytes: usize,
    pub decode: DecodeLimits,
    pub encode: EncodeLimits,
}

impl RpcLimits {
    /// Validate checked framing, backing and codec envelopes before constructing IO state.
    pub fn validate(self) -> Result<Self, BoundedErrorKind> {
        let blocks = self.blocks().ok_or(BoundedErrorKind::Configuration)?;
        let work =
            RecordBuilder::reservation_bound(blocks).ok_or(BoundedErrorKind::Configuration)?;
        if self.block_bytes == 0
            || self.max_record_bytes == 0
            || self.max_record_bytes > 0x7fff_ffff
            || self.max_fragments == 0
            || self.backing_bytes < self.max_record_bytes
            || self.max_builder_reservation_bytes < work
            || self.decode.max_depth == 0
            || self.decode.max_reservation_bytes == 0
            || self.encode.max_depth == 0
            || self.encode.max_wire_bytes == 0
            || self.encode.max_wire_bytes > self.max_record_bytes
            || self.encode.max_metadata_bytes == 0
            || self.encode.max_chunks == 0
            || self.encode.max_reservation_bytes == 0
            || self.decode.payload.max_logical_bytes > self.max_record_bytes
        {
            return Err(BoundedErrorKind::Configuration);
        }
        Ok(self)
    }

    fn blocks(self) -> Option<usize> {
        self.max_record_bytes
            .checked_add(self.block_bytes.checked_sub(1)?)?
            .checked_div(self.block_bytes)
    }
}

/// Bounded value-only diagnostics, except the IO cause may own a custom inner error.
#[derive(Debug)]
pub enum BoundedErrorKind {
    Configuration,
    Unusable,
    Argument,
    Admission,
    Framing,
    UnexpectedReply,
    Semantic,
    Io(std::io::Error),
    Storage(PayloadError),
    Encode(EncodeError),
    Decode(DecodeError),
    Rejected(rejected_reply),
    NotAccepted(accept_stat_data),
}

/// Explicit operation state, not a retry recommendation.
///
/// A WRITE with started IO and an unusable connection has an uncertain outcome.
#[derive(Debug)]
pub struct BoundedError {
    pub kind: BoundedErrorKind,
    pub io_started: bool,
    pub connection_usable: bool,
}

impl std::fmt::Display for BoundedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} (io_started={}, connection_usable={})",
            self.kind, self.io_started, self.connection_usable
        )
    }
}
impl std::error::Error for BoundedError {}

/// Runtime-neutral, single-flight bounded client. Cancellation after IO admission
/// poisons it; outputs and their backing charges may outlive the client.
pub struct BoundedRpcClient<IO> {
    io: IO,
    limits: RpcLimits,
    budget: BackingBudget,
    xid: u32,
    credential: opaque_auth<'static>,
    verifier: opaque_auth<'static>,
    usable: bool,
}

impl<IO> BoundedRpcClient<IO> {
    /// Construct with explicit policy and authentication; no IO is performed.
    pub fn new(
        io: IO,
        limits: RpcLimits,
        credential: opaque_auth<'static>,
        verifier: opaque_auth<'static>,
    ) -> Result<Self, BoundedErrorKind> {
        let limits = limits.validate()?;
        Ok(Self {
            io,
            limits,
            budget: BackingBudget::new(limits.backing_bytes),
            xid: fastrand::u32(..),
            credential,
            verifier,
            usable: true,
        })
    }

    #[must_use]
    pub const fn is_usable(&self) -> bool {
        self.usable
    }

    /// Reserved plus live backing bytes (not a whole-process measurement).
    #[must_use]
    pub fn backing_used(&self) -> usize {
        self.budget.used()
    }

    const fn error(&self, kind: BoundedErrorKind, io_started: bool) -> BoundedError {
        BoundedError {
            kind,
            io_started,
            connection_usable: self.usable,
        }
    }

    pub(super) const fn semantic_error(&mut self) -> BoundedError {
        self.usable = false;
        self.error(BoundedErrorKind::Semantic, true)
    }

    pub(super) const fn argument_error(&self) -> BoundedError {
        self.error(BoundedErrorKind::Argument, false)
    }

    pub(super) const fn limits(&self) -> RpcLimits {
        self.limits
    }

    pub(super) const fn validated(&mut self) {
        self.usable = true;
    }
}

impl<IO: AsyncRead + AsyncWrite> BoundedRpcClient<IO> {
    /// Structural generic RPC call. Full reply consumption is mandatory even for
    /// valid RPC rejection. No retries, reconnects or NFS semantic policy.
    pub async fn call<C: Encode + Sync, R: Decode>(
        &mut self,
        program: u32,
        version: u32,
        procedure: u32,
        args: &C,
    ) -> Result<R, BoundedError> {
        let value = self
            .call_unvalidated(program, version, procedure, args, self.limits.decode)
            .await?;
        self.validated();
        Ok(value)
    }

    // Successful structural decode stays poisoned until the caller's semantic
    // checks complete. There must be no intervening await before validated().
    pub(super) async fn call_unvalidated<C: Encode + Sync, R: Decode>(
        &mut self,
        program: u32,
        version: u32,
        procedure: u32,
        args: &C,
        decode: DecodeLimits,
    ) -> Result<R, BoundedError> {
        if !self.usable {
            return Err(self.error(BoundedErrorKind::Unusable, false));
        }
        let mut reservation = self
            .budget
            .try_reserve(self.limits.max_record_bytes)
            .map_err(|_| self.error(BoundedErrorKind::Admission, false))?;
        let header = rpc_msg {
            xid: self.xid,
            body: msg_body::CALL(call_body {
                rpcvers: RPC_VERSION_2,
                prog: program,
                vers: version,
                proc: procedure,
                cred: self.credential.borrow(),
                verf: self.verifier.borrow(),
            }),
        };
        let mut encoder = Encoder::new(self.limits.encode);
        encoder
            .encode(&header)
            .and_then(|()| encoder.encode(args))
            .map_err(|e| self.error(BoundedErrorKind::Encode(e), false))?;
        let plan = encoder
            .finish()
            .map_err(|e| self.error(BoundedErrorKind::Encode(e), false))?;
        if !plan.len().is_multiple_of(4) {
            return Err(self.error(BoundedErrorKind::Argument, false));
        }
        let xid = self.xid;
        self.xid = self.xid.wrapping_add(1);
        // No await before this point. Dropping an unpolled future changes nothing.
        self.usable = false;
        send(&mut self.io, &plan)
            .await
            .map_err(|e| self.error(BoundedErrorKind::Io(e), true))?;
        drop(plan);
        let record = receive(&mut self.io, self.limits, &mut reservation)
            .await
            .map_err(|e| self.error(e, true))?;
        drop(reservation);
        let result = decode_reply(record, decode, xid);
        match result {
            Ok(value) => Ok(value),
            Err((kind, valid)) => {
                self.usable = valid;
                Err(self.error(kind, true))
            }
        }
    }
}

fn decode_reply<R: Decode>(
    record: Payload,
    limits: DecodeLimits,
    xid: u32,
) -> Result<R, (BoundedErrorKind, bool)> {
    let mut decoder = Decoder::new(record, limits);
    let header: rpc_msg<'_, '_> = decoder
        .decode()
        .map_err(|e| (BoundedErrorKind::Decode(e), false))?;
    if header.xid != xid {
        return Err((BoundedErrorKind::UnexpectedReply, false));
    }
    let rejection = match header.body {
        msg_body::CALL(_) => return Err((BoundedErrorKind::UnexpectedReply, false)),
        msg_body::REPLY(reply_body::MSG_DENIED(reason)) => Some(BoundedErrorKind::Rejected(reason)),
        msg_body::REPLY(reply_body::MSG_ACCEPTED(reply)) => match reply.reply_data {
            accept_stat_data::SUCCESS => None,
            reason => Some(BoundedErrorKind::NotAccepted(reason)),
        },
    };
    if let Some(reason) = rejection {
        decoder
            .finish()
            .map_err(|e| (BoundedErrorKind::Decode(e), false))?;
        return Err((reason, true));
    }
    let value = decoder
        .decode()
        .map_err(|e| (BoundedErrorKind::Decode(e), false))?;
    decoder
        .finish()
        .map_err(|e| (BoundedErrorKind::Decode(e), false))?;
    Ok(value)
}

async fn receive<IO: AsyncRead>(
    io: &mut IO,
    limits: RpcLimits,
    reservation: &mut BackingReservation,
) -> Result<Payload, BoundedErrorKind> {
    nfs3_types::async_io::receive(
        io,
        RecordLimits {
            block_bytes: limits.block_bytes,
            max_record_bytes: limits.max_record_bytes,
            max_fragments: limits.max_fragments,
            max_builder_reservation_bytes: limits.max_builder_reservation_bytes,
        },
        reservation,
    )
    .await
    .map_err(|e| match e {
        FramingError::Io(e) => BoundedErrorKind::Io(e),
        FramingError::Limit => BoundedErrorKind::Framing,
        FramingError::Storage(e) => BoundedErrorKind::Storage(e),
    })
}
