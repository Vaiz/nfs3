# cargo-nfs3-server

`cargo-nfs3-server` is a simple and easy-to-deploy NFSv3 server designed for quick file transfers. Whether you need to share files temporarily or test NFSv3 functionality, this tool provides a lightweight and straightforward solution.

## Features
- **Quick Setup**: Start serving files with minimal configuration.
- **In-Memory Filesystem**: Option to use an in-memory filesystem for temporary file storage.
- **Read-Only Mode**: Serve files in a read-only mode for added safety.

## Limitations
- **Not Production-Ready**: This server is not designed for long-term or production use. Running it 24/7 may lead to unexpected issues.
- **No Security**: NFSv3 is an insecure protocol. All data transmitted is unencrypted and vulnerable to interception.

## Usage

### Installation

To use `cargo-nfs3-server`, ensure you have Rust installed. Then run cargo install

```bash
cargo install cargo-nfs3-server
```

### Running the Server
Every independent resource/deadline limit is required. There is no automatic
numeric profile or configuration file. This is an intentional finite **example**
for 1 MiB transfers, 16 connections and at most four admitted operations, not a
production recommendation or a process-memory cap:

```bash
cargo-nfs3-server --path ./shared --bind-ip 0.0.0.0 --bind-port 11111 \
  --record-block-bytes 65536 --record-max-bytes 2097152 --record-max-fragments 4096 \
  --decode-metadata-bytes 2097152 --decode-collection-elements 65536 \
  --decode-depth 64 --decode-reservation-bytes 16777216 \
  --encode-wire-bytes 2097152 --encode-metadata-bytes 2097152 --encode-chunks 256 \
  --encode-depth 64 --encode-reservation-bytes 16777216 \
  --read-max-bytes 1048576 --write-max-bytes 1048576 \
  --directory-wire-bytes 1048576 --directory-entries 10000 \
  --directory-name-bytes 4096 --directory-metadata-bytes 2097152 \
  --max-connections 16 --max-operations 4 --backing-bytes 536870912 \
  --idle-timeout-ms 30000 --progress-timeout-ms 5000 \
  --receive-timeout-ms 30000 --send-timeout-ms 30000
```

The [server policy contract](../nfs3_server/README.md#canonical-bounded-listener)
defines each domain, derived fixed work/builder envelopes, validation and exclusions.
All `*-bytes` values are byte counts; collection/entry/chunk/fragment/depth limits
are counts; deadline values are positive milliseconds. READ/WRITE transfer,
decode, encode, directory, connection, operation, backing and time limits remain
independent. Invalid/infeasible envelopes are rejected before binding.

The CLI uses the sole finite server engine. Ctrl-C stops admissions/readers and
gracefully drains admitted work and replies before releasing logging guards.
An uncooperative backend can prevent drain indefinitely; backend execution has
no timeout. Repeated XIDs execute again; a lost mutation reply is uncertain.
The crate's `tests/` retain mirror regressions and exercise required CLI flags;
the prebuilt `nfs3_tests` `test_mirror_fs` helper supplies explicit fixture policy.

### Options
- `--path`: Path to the directory to serve (required unless using `--memfs`).
- `--export-name`: Export path (default: `/`), normalized by the listener.
- `--bind-ip`: IP address to bind the server to (default: `0.0.0.0`).
- `--bind-port`: Port to bind the server to (default: `11111`).
- `--readonly`: Serve the directory as read-only.
- `--memfs`: Use an in-memory filesystem instead of a directory.
- `--log-level`: Set the log level (`error`, `warn`, `info`, `debug`, `trace`).
- `--log-file`: Path to a file for logging output.
- `--quiet`: Disable console logging.
- Operational policy flags: all flags shown in the example are required except
  the pre-existing path/bind options; see `--help` for names and types.
