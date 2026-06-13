# asansio
Async/await state machine for the Sans I/O design pattern.

[![crates.io](https://img.shields.io/crates/v/asansio.svg)](https://crates.io/crates/asansio)
[![docs.rs](https://img.shields.io/docsrs/asansio/latest)](https://docs.rs/asansio)

## The Idea

This crate is a experiment for Sans I/O in Rust (see [sans I/O for network
protocols](https://sans-io.readthedocs.io/) documentation to familiar with the
concept). Writing network protocol without performing I/O operations means
creating a state machine. Manually creating a state machine could be a tedious
process. As Rust async/await concept is an actual state machine with implicit
states created during compilation, this library is an experiment with using
async/await to provide such state machine automatically.  Let's check if this
is good solution to the problem.

## Dependency

It is `no_std` crate without allocations on the heap. It depends only on the
`core`, no other crates. Only examples uses `std`, `clap` and `tokio` as
`dev-dependencies`.

## Usage

```rust
struct Request([u8; 10]);
struct Response([u8; 20]);

async fn sans_task(sans: Sans<Request, Response>) {
    let mut request_buf = [1u8; 10];
    let response = sans.handle(Request(request_buf)).await;
    assert_eq!(response.0, [2; 20]);

    request_buf.fill(3);
    let response = sans.handle(Request(request_buf)).await;
    assert_eq!(response.0, [4; 20]);
}

let (sans, io) = asansio::new();

let task = pin!(sans_task(sans));

let (handle, request) = io.start(task).unwrap().unwrap();
assert_eq!(request.0, [1; 10]);

let mut response_buf = [2; 20];
let (handle, request) = io.handle(handle, Response(response_buf)).unwrap().unwrap();
assert_eq!(request.0, [3; 10]);

response_buf.fill(4);
assert!(io.handle(handle, Response(response_buf)).unwrap().is_none());
```

The crate divides a problem into two parts. The first `Sans` takes care of the
state machine independent of the I/O and the second `Io` is responsible with
I/O communication.  These two parts communicate using `Request` and `Respond`
types, which are defined by the user (for real scenarios they could be
`enums`).

See also more [examples](examples).

## License

Licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
