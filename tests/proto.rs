use sansio::Id;
use sansio::SansIoHandler;
use sansio::SansIoMachine;
use std::time::Duration;
use std::time::Instant;

pub struct Proto {}

impl Proto {
    pub fn new() -> Self {
        Self {}
    }
}

impl SansIoMachine for Proto {
    type Request = Request;
    type Response = Response;
    type Error = ();

    async fn start(&self, sansio: &mut SansIoHandler<Request>) {}

    async fn handle(&self, sansio: &mut SansIoHandler<Request>, id: Id, response: Response) {}
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
