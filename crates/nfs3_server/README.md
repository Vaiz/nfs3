Disclaimer
==========

This project originated as a fork of [xetdata/nfsserve](https://github.com/xetdata/nfsserve) and
includes a substantial amount of code from that repository.

Rust `NFSv3` Server
===================

This is an incomplete but very functional implementation of an `NFSv3` server in Rust. The main goal of this project is to enable robust and flexible data sharing. By implementing the provided traits, you can share almost any type of data, ranging from a regular filesystem to more complex storage backends like S3 buckets (note: S3 integration is not included in this library).

Quick start
===========

If you want to try the server out, you can use [cargo-nfs3-server](https://github.com/Vaiz/nfs3/blob/main/crates/cargo_nfs3_server/README.md) crate that provides a cli tool able to run in memory filesystem or mirror an existing the local one.

Examples
========

The project includes a set of examples that can be run pretty much in the same way. You can find them in [examples](https://github.com/Vaiz/nfs3/tree/main/crates/nfs3_server/examples) folder.

`MemFs` example
---------------

```bash
cargo run --example memfs --features memfs
```

To mount. On Linux (sudo may be required):
```bash
mkdir demo
mount.nfs -o user,noacl,nolock,vers=3,tcp,wsize=1048576,rsize=131072,actimeo=120,port=11111,mountport=11111 localhost:/ demo
```

On Mac:
```bash
mkdir demo
mount_nfs -o nolocks,vers=3,tcp,rsize=131072,actimeo=120,port=11111,mountport=11111 localhost:/ demo
```

On Windows (Pro required as Home does not have NFS client):
```bash
mount.exe -o anon,nolock,mtype=soft,fileaccess=6,casesensitive,lang=ansi,rsize=128,wsize=128,timeout=60,retry=2 \\127.0.0.1\\ X:
```

Note that this example uses *writable* instance of `MemFs`.

Usage
=====

You simply need to implement the `vfs::NfsReadFileSystem` and (optionaly) `vfs::NfsFileSystem`
traits. See memfs.rs for an example and bin/main.rs for how to actually start
a service. The interface generally not difficult to implement; demanding mainly
the ability to associate every file system object (directory/file) with a 64-bit
ID. Directory listing can be a bit complicated due to the pagination requirements.

Canonical bounded listener
==========================

`tcp::NFSTcpListener::bind(address, vfs, export, limits)` requires
`vfs::NfsFileSystem`; `bind_ro` wraps an `NfsReadFileSystem` backend. Supply every
`ServerLimits`, codec, framing and directory limit explicitly: there are no
default profiles. `validate` checks the startup envelope and fixed maximum
READ/WRITE reply codec work and worst-case directory metadata/reservation work
before binding, without allocating transfer-sized backing.
`DecodePolicy` and `EncodePolicy` retain independent codec limits. Request bulk
logical/backing/segment limits derive from the admitted record, never the WRITE
semantic size; reply retained bulk capacity derives from supplied READ storage.
Builder work and the private control floor are computed internally, not caller knobs.
`accept` returns an optional connection task handle; dropping that handle
detaches it. `run` accepts until error/shutdown. `drain` stops admissions and
readers and waits for admitted work and bounded reply sends; shutdown does not
cancel their replies. `run` returns `Interrupted` on shutdown, even without a
new socket.
Export names normalize to one leading slash and no trailing slash before the
name-length check. Accepted sockets retain `TCP_NODELAY = true`.
This is the sole server engine. The old `NFSTcp` helper trait, old TCP/rpcwire
processing and duplicate-XID tracker are removed. Use `local_addr` instead of
`get_listen_ip/port`, pass export/policy at binding instead of `with_export_name`,
and use `run`/`drain` instead of `handle_forever`. There is no mount-notification
setter, generation-specific bind or `"auto"` address search; supply a socket
address. No compatibility wrapper or copying bridge is provided.
The [CLI](../cargo_nfs3_server/README.md) requires independent policy flags.
Examples intentionally choose finite constants in `examples/support/mod.rs`;
they are demonstration policy, not library or CLI defaults.
Correctness checks do not establish a copy, memory or performance target pass.

The bounded listener serves the same implemented NFS/mount/portmap procedures
through shared metadata handlers. IO uses the runtime-neutral
`nfs3_types::async_io` framing core, not a server-to-client dependency.
There is one operation/reply at a time per connection, with independent
connections progressing concurrently. It reads only the first four-byte
marker before failfast operation/domain admission. No body read or mutation
occurs on admission exhaustion. Connection exhaustion closes immediately.
Unread pipelined input can cause a TCP reset and discard even an earlier reply.
Repeated XIDs execute again: there is no duplicate suppression or exactly-once
guarantee. Invalid XDR, auth, padding and trailing input are checked strictly.
The workspace duplex harness uses this same connection loop through the hidden
test-only exports and explicitly supplies its finite fixture limits.

The per-operation reservation is the **sum** of maximum request backing,
maximum READ backing, builder work, decode work/metadata, encode work/metadata,
directory host expansion and control entitlement. Directory expansion includes
the minimum wire-entry count, actual host entry layout, geometric vector work
and conservative old/new overlap; names and handles have a separate allowance.
`operation_envelope` exposes this checked sum. No operation requests another
global grant after admission. Allocations grow incrementally, not to an
advertised fragment length. Spent backing tickets release to the domain, not
back into the reservation; immutable origins remain charged until their final
owner drops. Unused entitlement and the operation slot survive through actual
work and reply send/drop.

This bounds managed backing capacity and requested codec/control allocation
work, **not total process memory**. Runtime/task/allocator overhead, persistent
filesystem data, custom backend allocations and backend-created directory
iterators/name capacity before transfer are separate domains. Selected names
are capacity-checked before server vector growth. Backends must honor the
documented `FileHandle` size contract. `AUTH_SYS` has a separate RFC-bounded
machine/group decode allowance included in control, not an unrestricted second
record decode budget. READLINK output is checked before server ownership/copy.

The two VFS traits have only owned bulk methods:
`NfsReadFileSystem::read(OwnedRead)` and `NfsFileSystem::write(OwnedWrite)`.
The separate `NfsOwnedRead`/`NfsOwnedIo` traits and Vec/slice methods are removed.
`OwnedRead` supplies charged initialized writable storage before backend work.
`complete(count, eof)` consumes it into a checked prefix; a backend cannot
substitute an uncharged Vec. `OwnedWrite` supplies an owned segmented Payload,
handle, offset and requested stability. Both privately retain the work lease.
Custom backends must move the **whole request** into actual blocking jobs and
keep it for all actual work, including error paths; spawning unrelated borrowed
work violates this contract. No blanket legacy Vec/slice copying adapter exists.
`MemFs` fills supplied READ storage and iterates WRITE chunks. The mirror backends
implement these methods using `fs_util` standard-file blocking jobs. Those jobs
use `sync_data`/`sync_all` for `DATA_SYNC`/`FILE_SYNC`, and COMMIT flushes the file.
OS synchronization is not a claim about storage hardware crash guarantees.

RPC/auth/arguments are decoded and the record exactly finished/dropped before
the first backend await. WRITE retains only selected origins. Local WRITE
policy checks ROFS, then count mismatch/transfer limit, then handle/preattrs.
READ and directory counts are clamped; FSINFO and PATHCONF advertise the
configured supported transfers and name limit. Directory exhaustion preserves
the last included cookie with eof=false, or returns TOOSMALL if none fits.
Existing cookie-verifier behavior is unchanged.
Well-formed over-limit name arguments return procedure-specific NFS NAMETOOLONG
failures, without backend calls. The configured name/host/page limits must fit
the backend's existing filenames, not just newly created names: an over-limit
first directory entry produces TOOSMALL and retrying cannot advance it. The
mirror cache retains one yielded name/handle per cached iterator (a backend
allocation domain) so an unsent lookahead can be replayed from the last sent
cookie; that name is cloned when yielded/replayed. Popping either resume cookie
consumes the same cached state, not a second independent iterator.

Idle, per-IO progress, whole-record receive and whole-reply send deadlines are
positive and explicit. Partial markers use progress/whole-record deadlines,
not repeated idle timeouts. Backend execution has **no timeout**.
Admitted operations run in detached server-owned tasks with one-result
oneshots; disconnect, connection-task cancellation and listener drop do not
abort their backend work. Dropping the listener signals readers. Drain can
remain pending for an uncooperative backend.

XIDs correlate replies only. There is no bounded-path transaction suppression,
retry cache, digest, replay or exactly-once guarantee; repeated complete records
may execute again. Admitted WRITE continues after disconnect. A lost
mutation reply (including encode/send failure) is **uncertain**, never evidence
that retry is safe. Post-mutation encode failure closes rather than fabricating
a failure status; a nonmutating encode failure can use a fixed pre-reserved RPC
error. Process/runtime termination is outside the disconnect guarantee.

Relevant RFCs
=============
 - XDR is the message format: [RFC 1014](https://datatracker.ietf.org/doc/html/rfc1014).
 - SUN RPC is the RPC wire format: [RFC 1057](https://datatracker.ietf.org/doc/html/rfc1057).
 - NFS is at [RFC 1813](https://datatracker.ietf.org/doc/html/rfc1813).
 - NFS Mount Protocol is at [RFC 1813 Appendix I](https://datatracker.ietf.org/doc/html/rfc1813#appendix-I).
 - `PortMapper` is at [RFC 1057 Appendix A](https://datatracker.ietf.org/doc/html/rfc1057#appendix-A).


More More Details Than Necessary
================================
The basic way a message works is:
1. We read a collection of fragments off a TCP stream 
   (a 4 byte length header followed by a bunch of bytes)
2. We assemble the fragments into a record
3. The Record is of a SUN RPC message type.
4. A message tells us 3 pieces of information,
  - The RPC Program (just an integer denoting
    a protocol "class". For instance NFS protocol is 100003, the Portmapper protocol is 100000).
  - The version of the RPC program (ex: 3 = `NFSv3`, 4 = `NFSv4`, etc)
  - The method invoked (Which NFS method to call) (See for instance nfs.rs top comment for the list)
5. Continuing to decode the message will give us the arguments of the method
6. And we take the method response, wrap it around a record and return it. 

Portmapper
----------
First, lets get portmapper out of the way. This is a *very* old mechanism which
is rarely used anymore. The portmapper is a daemon which runs on a machine running
on port 111. When NFS, or other RPC services start, they register with the 
portmapper service with the port they are listening on (Say NFS on 2049). 
Then when another machine wants to connect to NFS, they first ask the port mapper
on 111 to ask about which port NFS is listening on, then connects to the returned 
port.

We do not strictly need to implement this protocol as this is pretty much
unused these days (`NFSv4` does not use the portmapper for instance). If `-o port` and `-o mountport`
are specified, Linux and Mac's builtin NFS client do not need it either.
But this was useful for debugging and testing as libnfs seems to require a
portmapper, but it annoyingly hardcodes it to 111. I modified the source to
change it to 12000 for testing and implemented the one `PMAPPROC_GETPORT`
method so I can test with libnfs.


NFS Basics
==========
The way NFS works is that every file system object (dir/file/symlink) has 2
ways in which it can be addressed:

1. `fileid3: u64` . A 64-bit integer. Equivalent to an inode number.
2. `nfs_fh3`: A variable opaque object up to 64 bytes long.

Basically anytime the client tries to access any information about an object,
it needs an `nfs_fh3`. The purpose of the `nfs_fh3` serves 2 purposes:

 - Allow server to cache additional query information in the handle that may exceed
   64-bit. For instance if the server has multiple exports on different disk volumes,
   I may need a few more bits to identify the disk volume.
 - Allow client to identify when server has "restarted" and thus client has to
   clear all caches. the `nfs_fh3` handle should contain a token that is unique
   to when the NFS server first started up which allows the server to check that
   the handle is still valid. If the server has restarted, all previous handles
   will therefore be "expired" and any usage of them should trigger a handle expiry
   error informing the clients to expunge all caches.


However, the only way to obtain an `nfs_fh3` for a file is via directory traversal.
i.e. There is a lookup method 
`LOOKUP(directory's handle, filename of file/dir in directory)` 
which returns the handle for the filename.

For instance to get the handle of a file "dir/a.txt", I first need the handle
for the directory "dir/", then query `LOOKUP(handle, "a.txt")`.

The question is then, how do I get my first handle? That is what the MOUNT
protocol addresses.

Mount
-----
The MOUNT protocol provides a list of "exports", (in the simplest case. Just "/")
and the client will request to MNT("/") which will return the handle of this 
root directory.

Normally the server can and do maintain a list of mounts which can be queried,
and really the client can UMNT (unmount) as well.  But in our case we
only implement MNT and EXPORT which suffices. NFS clients generally
ignore the return message of UMNT as there is really nothing the
client can do on a UMNT failure. As such our Mount protocol implementation
is entirely stateless.

NFS
---
The NFS protocol itself is pretty straightforward with most annoyances
due to handling of the XDR messaging format (in paticular with optional,
lists, etc).

What is nice is that the design of NFS is completely stateless. It is mostly
sit down and implement all the methods that are hit and test them against a 
client.
