# IFT3325 - Devoir 2

![rust tests workflow](https://github.com/etiennecollin/ift3325/actions/workflows/rust.yml/badge.svg)

## Dependencies

- [rust](https://www.rust-lang.org/tools/install) >= 1.82.0

> [!NOTE]
> The code was tested using `rustc 1.82.0`.

## Usage

Run the server:

```bash
./server <port_number> <prob_frame_drop> <prob_bit_flip>
```

Run the client:

```bash
./client <server_address> <server_port> <file> <go_back_n> <prob_frame_drop> <prob_bit_flip>
```

> [!NOTE]
> If the file sent only contains the string `shutdown`, the server will shutdown.

> [!NOTE]
> The probabilities are given as floating point numbers in the range [0, 1]
> and are independent probabilities.

<!-- Run the tunnel: -->
<!---->
<!-- ```bash -->
<!-- ./tunnel <in_port> <out_address> <out_port> <prob_frame_drop> <prob_bit_flip> -->
<!-- ``` -->
<!---->
<!-- > [!NOTE] -->
<!-- > The tunnel allows the simulation of a noisy environment where frames -->
<!-- > might be dropped or suffer bit flips. The probabilities in the arguments are -->
<!-- > values in the range \[0, 1\] and are independant. Every frame, in any direction, -->
<!-- > has a probability of being dropped. If it is not dropped, the second -->
<!-- > probability is used to determine if a bit is flipped in the frame. -->

## Example usage

In this example, the client and server are used. Each frame from the server
has a 10% probability of being dropped and a 10% probability of suffering a
bit flip.

```bash
cargo run --release -p server 8080 0.1 0.1
cargo run --release -p client 127.0.0.1 8080 <file_path> 0 0 0
```

## Testing

To run the tests, use:

```bash
cargo test
```

For a better test UI and parallel execution of tests, use `cargo-nextest`.
Install it with:

```bash
cargo install cargo-nextest --locked
```

and run the tests with:

```bash
cargo nextest run
```

## Logging

The log level may be changed through an environment variable.

```bash
RUST_LOG=<log_level>
```

where `<log_level>` is, in increasing order of severity, either:

- `DEBUG`
- `INFO`
- `WARN`
- `ERROR`

By default, the log level used is `INFO`.

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

---

> [!NOTE]
> The code is available at https://github.com/etiennecollin/ift3325
