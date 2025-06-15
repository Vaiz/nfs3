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
cargo install cargo-nfs3-server@0.1.0-alpha.2
```

### Running the Server
You can start the server with the following command:

```bash
cargo-nfs3-server --path <directory-to-serve> --bind-ip <ip-address> --bind-port <port>
```

#### Example:
```bash
cargo-nfs3-server --path ./shared --bind-ip 0.0.0.0 --bind-port 11111
```

### Options
- `--path`: Path to the directory to serve (required unless using `--memfs`).
- `--bind-ip`: IP address to bind the server to (default: `0.0.0.0`).
- `--bind-port`: Port to bind the server to (default: `11111`).
- `--readonly`: Serve the directory as read-only.
- `--memfs`: Use an in-memory filesystem instead of a directory.
- `--log-level`: Set the log level (`error`, `warn`, `info`, `debug`, `trace`).
- `--log-file`: Path to a file for logging output.
- `--quiet`: Disable console logging.
