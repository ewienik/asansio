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

You can provide async iface trait that can handle all SansIo handling. This way
you can use single protocol defined in async way with std or async runtime.

This is the example of std interface and client:

```rust
use asansio::Io;
use asansio::IoHandle;
use asansio::Sans;
use std::pin::Pin;

pub trait Iface {
    async fn recv(&mut self) -> Option<usize>;
    async fn send(&mut self, value: usize) -> Option<()>;
}

async fn run(mut iface: impl Iface) -> Option<()> {
    loop {
        let value = iface.recv().await?;
        iface.send(value + 1).await?;
    }
}

type Request = Option<usize>;
type Response = Option<usize>;

struct IfaceStd {
    response: Option<usize>,
    sans: Sans<Request, Response>,
}

impl Iface for IfaceStd {
    async fn recv(&mut self) -> Option<usize> {
       if let Some(response) = self.response.take() {
           return Some(response);
       }
       self.sans.handle(None).await
    }

    async fn send(&mut self, value: usize) -> Option<()> {
       self.response = self.sans.handle(Some(value)).await;
       self.response.map(|_| ())
    }
}

struct ClientStd {
    io: Io<Request, Response>,
    handle: Option<IoHandle<Request, Response, Box<dyn Future<Output = ()>>>>,
}

impl ClientStd {
    fn new() -> Option<Self> {
        let (sans, io) = asansio::new::<Request, Response>();
        let proto = Box::pin({
            async move {
                run(IfaceStd { response: None, sans }).await.unwrap_or(());
            }
        }) as Pin<Box<dyn Future<Output = ()>>>;
        let (handle, request) = io.start(proto).ok().flatten()?;
        assert_eq!(request, None);
        let handle = Some(handle);
        Some(Self { io, handle })
    }

   fn call(&mut self, value: usize) -> Option<usize> {
       let handle = self.handle.take().unwrap();
       let (handle, request) = self.io.handle(handle, Some(value)).unwrap()?;
       self.handle = Some(handle);
       request
   }
}

let mut client = ClientStd::new().unwrap();

assert_eq!(client.call(0), Some(1));
assert_eq!(client.call(2), Some(3));
assert_eq!(client.call(8), Some(9));
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
