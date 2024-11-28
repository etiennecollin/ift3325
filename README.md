# IFT3325 - Devoir 2

![rust tests workflow](https://github.com/etiennecollin/ift3325/actions/workflows/rust.yml/badge.svg)

## Dependencies

- [Rust](https://www.rust-lang.org/tools/install)

> [!NOTE]
> The code was tested using `rustc 1.82.0`.

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
> values in the range \[0, 1\] and are independant. Every frame, in any direction,
> has a probability of being dropped. If it is not dropped, the second
> probability is used to determine if a bit is flipped in the frame.

## Example usage

In this example, the client, tunnel and server will be used. Each frame, in
any direction, has a 10% probability of being dropped and a 10% probability
of suffering a bit flip.

```bash
cargo run --release -p server 8080
cargo run --release -p tunnel 8081 127.0.0.1 8080 0.1 0.1
cargo run --release -p client 127.0.0.1 8081 <file_path> 0
```

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
