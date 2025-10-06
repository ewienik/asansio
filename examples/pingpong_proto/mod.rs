use asansio::Sans;
use std::time::Duration;

pub enum ClientRequest<'a> {
    Ready,
    WriteMessage { payload: &'a [u8] },
    WriteSleep { payload: &'a [u8] },
    Message { msg: &'a str },
}

pub enum ClientResponse<'a> {
    ReadMessage { payload: &'a [u8] },
    Message { msg: &'a str },
    Sleep { duration: Duration },
}

pub enum ServerRequest<'a> {
    Read,
    WriteMessage { payload: &'a [u8] },
    Sleep { duration: Duration },
}

pub enum ServerResponse<'a> {
    ReadMessage { payload: &'a [u8] },
    ReadSleep { payload: &'a [u8] },
}

struct Cache {
    read: Vec<u8>,
}

impl Cache {
    fn new() -> Self {
        Self { read: Vec::new() }
    }
}

fn client_read_message<'a>(payload: &'a [u8]) -> ClientRequest<'a> {
    ClientRequest::Message {
        msg: str::from_utf8(payload).unwrap_or("(wrong utf-8 encoding)"),
    }
}

fn client_message<'a>(msg: &'a str) -> ClientRequest<'a> {
    ClientRequest::WriteMessage {
        payload: msg.as_bytes(),
    }
}

fn client_sleep<'a>(cache: &'a mut Cache, duration: Duration) -> ClientRequest<'a> {
    cache.read.clear();
    cache
        .read
        .push(duration.as_millis().clamp(0, u8::max_value() as u128) as u8);
    ClientRequest::WriteSleep {
        payload: cache.read.as_slice(),
    }
}

pub async fn run_client<'a>(sans: Sans<ClientRequest<'a>, ClientResponse<'a>>) {
    let mut cache = Cache::new();

    let mut sans_resp = sans.start(&ClientRequest::Ready).await;
    loop {
        let request = match sans_resp.response() {
            Some(ClientResponse::ReadMessage { payload }) => client_read_message(payload),
            Some(ClientResponse::Message { msg }) => client_message(msg),
            Some(ClientResponse::Sleep { duration }) => client_sleep(&mut cache, *duration),
            None => break,
        };
        sans_resp = sans.handle(sans_resp, &request).await;
    }
}

fn server_read_message<'a>(cache: &'a mut Cache, payload: &[u8]) -> ServerRequest<'a> {
    cache.read.clear();
    cache.read.extend_from_slice(b"Received: ");
    cache.read.extend_from_slice(payload);
    ServerRequest::WriteMessage {
        payload: cache.read.as_slice(),
    }
}

fn server_read_sleep<'a>(payload: &[u8]) -> ServerRequest<'a> {
    if payload.len() == 1 {
        let duration = Duration::from_millis(payload[0] as u64);

        ServerRequest::Sleep { duration }
    } else {
        ServerRequest::Read
    }
}

pub async fn run_server<'a>(sans: Sans<ServerRequest<'a>, ServerResponse<'a>>) {
    let mut cache = Cache::new();

    let mut sans_resp = sans.start(&ServerRequest::Read).await;
    loop {
        let request = match sans_resp.response() {
            Some(ServerResponse::ReadMessage { payload }) => {
                server_read_message(&mut cache, payload)
            }
            Some(ServerResponse::ReadSleep { payload }) => server_read_sleep(payload),
            None => break,
        };
        sans_resp = sans.handle(sans_resp, &request).await;
    }
}
