use asansio::Sans;
use std::collections::VecDeque;

pub enum ClientRequest<'a> {
    ReadPayload,
    WritePayload { payload: &'a [u8] },
    Read { tag: u8, val: &'a [u8] },
}

pub enum ClientResponse<'a> {
    ReadPayload { payload: &'a [u8] },
    Write { tag: u8, val: &'a [u8] },
}

pub enum ServerRequest<'a> {
    ReadPayload,
    WritePayload { payload: &'a [u8] },
    Read { tag: u8, val: &'a [u8] },
}

pub enum ServerResponse<'a> {
    ReadPayload { payload: &'a [u8] },
    Write { tag: u8, val: &'a [u8] },
}

struct Cache {
    write: Vec<u8>,
    read: VecDeque<u8>,
    read_consumed: usize,
}

impl Cache {
    fn new() -> Self {
        Self {
            write: Vec::new(),
            read: VecDeque::new(),
            read_consumed: 0,
        }
    }

    fn read_packet(&mut self, payload: &[u8]) -> Option<&[u8]> {
        self.read.drain(..self.read_consumed);
        self.read_consumed = 0;

        payload.iter().for_each(|byte| self.read.push_back(*byte));

        if self.read.len() < 2 {
            return None;
        }

        let len = self.read[1] as usize;
        if self.read.len() < len + 2 {
            return None;
        }

        self.read_consumed = len + 2;
        Some(&self.read.make_contiguous()[..self.read_consumed])
    }

    fn write_packet(&mut self, tag: u8, val: &[u8]) -> &[u8] {
        self.write.clear();
        self.write.push(tag);
        self.write.push(val.len() as u8);
        self.write.extend_from_slice(val);
        &self.write[0..val.len() + 2]
    }
}

fn client_read_payload<'a>(cache: &'a mut Cache, payload: &[u8]) -> ClientRequest<'a> {
    if let Some(buf) = cache.read_packet(payload) {
        ClientRequest::Read {
            tag: buf[0],
            val: &buf[2..],
        }
    } else {
        ClientRequest::ReadPayload
    }
}

fn client_write<'a>(cache: &'a mut Cache, tag: u8, val: &[u8]) -> ClientRequest<'a> {
    ClientRequest::WritePayload {
        payload: cache.write_packet(tag, val),
    }
}

pub async fn run_client<'a>(sans: Sans<ClientRequest<'a>, ClientResponse<'a>>) {
    let mut cache = Cache::new();

    let mut sans_resp = sans.start(&ClientRequest::ReadPayload).await;
    loop {
        let request = match sans_resp.response() {
            Some(ClientResponse::ReadPayload { payload }) => {
                client_read_payload(&mut cache, payload)
            }
            Some(ClientResponse::Write { tag, val }) => client_write(&mut cache, *tag, val),
            None => break,
        };
        sans_resp = sans.handle(sans_resp, &request).await;
    }
}

fn server_read_payload<'a>(cache: &'a mut Cache, payload: &[u8]) -> ServerRequest<'a> {
    if let Some(buf) = cache.read_packet(payload) {
        ServerRequest::Read {
            tag: buf[0],
            val: &buf[2..],
        }
    } else {
        ServerRequest::ReadPayload
    }
}

fn server_write<'a>(cache: &'a mut Cache, tag: u8, val: &[u8]) -> ServerRequest<'a> {
    ServerRequest::WritePayload {
        payload: cache.write_packet(tag, val),
    }
}

pub async fn run_server<'a>(sans: Sans<ServerRequest<'a>, ServerResponse<'a>>) {
    let mut cache = Cache::new();

    let mut sans_resp = sans.start(&ServerRequest::ReadPayload).await;
    loop {
        let request = match sans_resp.response() {
            Some(ServerResponse::ReadPayload { payload }) => {
                server_read_payload(&mut cache, payload)
            }
            Some(ServerResponse::Write { tag, val }) => server_write(&mut cache, *tag, val),
            None => break,
        };
        sans_resp = sans.handle(sans_resp, &request).await;
    }
}
