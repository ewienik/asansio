use sansio::Handler;
use std::time::Duration;
use std::time::Instant;

pub struct Proto {}

impl Proto {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn start(&self, _handler: Handler<Request, Response>) {}
}

pub enum Request {
    Sleep(Sleep),
    Read(Read),
    Write(Write),
}

pub enum Response {
    Now(Now),
    Read(Read),
}

pub struct Sleep {
    duration: Duration,
}

pub struct Now {
    now: Instant,
}

pub struct Read;

pub struct Write {
    now: Instant,
    payload: Vec<u8>,
}
