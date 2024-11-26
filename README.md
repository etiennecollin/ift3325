# IFT3325 - Devoir 2

![rust tests workflow](https://github.com/etiennecollin/ift3325/actions/workflows/rust.yml/badge.svg)

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

Run the tunnel:

```bash
./tunnel <in_port> <out_address> <out_port> <prob_frame_drop> <prob_bit_flip>
```

> [!NOTE]
> The tunnel allows the simulation of a noisy environment where frames
> might be dropped or suffer bit flips. The probabilities in the arguments are
> values in the range \[0, 1\].

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
- `tunnel`
