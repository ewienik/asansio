use bytes::Bytes;
use bytes::BytesMut;
use sansio::Sans;
use std::time::Duration;

pub enum ClientRequest {
    Ready,
    WriteMessage { payload: Bytes },
    WriteSleep { payload: Bytes },
    Message { msg: String },
}

pub enum ClientResponse {
    ReadMessage { payload: Bytes },
    Message { msg: String },
    Sleep { duration: Duration },
}

pub enum ServerRequest {
    Read,
    WriteMessage { payload: Bytes },
    Sleep { duration: Duration },
}

pub enum ServerResponse {
    ReadMessage { payload: Bytes },
    ReadSleep { payload: Bytes },
}

fn client_read_message(payload: &Bytes) -> ClientRequest {
    ClientRequest::Message {
        msg: String::from_utf8(payload.to_vec()).unwrap_or("(wrong utf-8 encoding)".to_string()),
    }
}

fn client_message(msg: &String) -> ClientRequest {
    ClientRequest::WriteMessage {
        payload: msg.clone().into(),
    }
}

fn client_sleep(duration: Duration) -> ClientRequest {
    ClientRequest::WriteSleep {
        payload: Bytes::copy_from_slice(&[
            duration.as_millis().clamp(0, u8::max_value() as u128) as u8
        ]),
    }
}

pub async fn run_client(sans: Sans<ClientRequest, ClientResponse>) {
    let mut sans_resp = sans.start(&ClientRequest::Ready).await;
    loop {
        let request = match sans_resp.response() {
            Some(ClientResponse::ReadMessage { payload }) => client_read_message(payload),
            Some(ClientResponse::Message { msg }) => client_message(msg),
            Some(ClientResponse::Sleep { duration }) => client_sleep(*duration),
            None => break,
        };
        sans_resp = sans.handle(sans_resp, &request).await;
    }
}

fn server_read_message(payload: &Bytes) -> ServerRequest {
    let mut buf = BytesMut::new();
    buf.extend_from_slice(b"Received: ");
    buf.extend_from_slice(payload);
    ServerRequest::WriteMessage {
        payload: buf.freeze(),
    }
}

fn server_read_sleep(payload: &Bytes) -> ServerRequest {
    if payload.len() == 1 {
        let duration = Duration::from_millis(payload[0] as u64);

        ServerRequest::Sleep { duration }
    } else {
        ServerRequest::Read
    }
}

pub async fn run_server(sans: Sans<ServerRequest, ServerResponse>) {
    let mut sans_resp = sans.start(&ServerRequest::Read).await;
    loop {
        let request = match sans_resp.response() {
            Some(ServerResponse::ReadMessage { payload }) => server_read_message(payload),
            Some(ServerResponse::ReadSleep { payload }) => server_read_sleep(payload),
            None => break,
        };
        sans_resp = sans.handle(sans_resp, &request).await;
    }
}
