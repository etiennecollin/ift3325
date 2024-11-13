# IFT3325 - Devoir 2

## Usage

Run the server:

```bash
./server <port>
```

Run the client:

```bash
./client <address> <port> <file_path> <go_back_n>
```

> [!NOTE]
> If the file sent only contains the string `shutdown`, the server will shutdown.

## Debugging vs. Release

Use the `--profile dev` flag for a debug build, and the `--release` flag for a release build:

```bash
# Debug
cargo run --profile dev -p <target> <args>
# Release
cargo run --release -p <target> <args>
```

`<args>` are the args for the executable and `<target>` is either:

- `server`
- `client`
