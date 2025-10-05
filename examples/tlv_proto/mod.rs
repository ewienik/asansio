use asansio::Sans;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

pub enum ClientRequest {
    ReadPayload,
    WritePayload { payload: Bytes },
    Read { tag: u8, val: Bytes },
}

pub enum ClientResponse {
    ReadPayload { payload: Bytes },
    Write { tag: u8, val: Bytes },
}

pub enum ServerRequest {
    ReadPayload,
    WritePayload { payload: Bytes },
    Read { tag: u8, val: Bytes },
}

pub enum ServerResponse {
    ReadPayload { payload: Bytes },
    Write { tag: u8, val: Bytes },
}

struct Cache {
    write: BytesMut,
    read: BytesMut,
}

impl Cache {
    fn new() -> Self {
        Self {
            write: BytesMut::new(),
            read: BytesMut::new(),
        }
    }

    fn read_packet(&mut self, payload: &Bytes) -> Option<Bytes> {
        self.read.extend_from_slice(payload);

        if self.read.len() < 2 {
            return None;
        }

        let len = self.read[1] as usize;
        if self.read.len() < len + 2 {
            return None;
        }

        Some(self.read.split_to(len + 2).freeze())
    }

    fn write_packet(&mut self, tag: u8, val: &Bytes) -> Bytes {
        self.write.put_u8(tag);
        self.write.put_u8(val.len() as u8);
        self.write.extend_from_slice(val);
        self.write.split().freeze()
    }
}

fn client_read_payload(cache: &mut Cache, payload: &Bytes) -> ClientRequest {
    if let Some(mut buf) = cache.read_packet(payload) {
        ClientRequest::Read {
            tag: buf[0],
            val: buf.split_off(2),
        }
    } else {
        ClientRequest::ReadPayload
    }
}

fn client_write(cache: &mut Cache, tag: u8, val: &Bytes) -> ClientRequest {
    ClientRequest::WritePayload {
        payload: cache.write_packet(tag, val),
    }
}

pub async fn run_client(sans: Sans<ClientRequest, ClientResponse>) {
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

fn server_read_payload(cache: &mut Cache, payload: &Bytes) -> ServerRequest {
    if let Some(mut buf) = cache.read_packet(payload) {
        ServerRequest::Read {
            tag: buf[0],
            val: buf.split_off(2),
        }
    } else {
        ServerRequest::ReadPayload
    }
}

fn server_write(cache: &mut Cache, tag: u8, val: &Bytes) -> ServerRequest {
    ServerRequest::WritePayload {
        payload: cache.write_packet(tag, val),
    }
}

pub async fn run_server(sans: Sans<ServerRequest, ServerResponse>) {
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
