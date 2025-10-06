# sansio
Async/await state machine for the sans-io design pattern.

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

## License

Licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
