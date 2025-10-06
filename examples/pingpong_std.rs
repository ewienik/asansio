mod tlv_pingpong_proto;

use clap::Parser;
use std::io::Read;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::pin::pin;
use std::time::Duration;
use tlv_pingpong_proto::ClientRequest;
use tlv_pingpong_proto::ClientResponse;

#[derive(Parser)]
#[clap(version)]
struct Args {
    #[clap(short, long, default_value = "127.0.0.1:7123")]
    connect: SocketAddr,
}

fn main() {
    client(Args::parse().connect);
}

fn client(connect: SocketAddr) {
    let tcp = TcpStream::connect(connect).unwrap();
    println!("Connected to {}", connect);
    client_process(tcp);
}

struct Cache {
    buf: Vec<u8>,
    msg: String,
    count_read: usize,
    count_send: usize,
    count_recv: usize,
}

impl Cache {
    fn new() -> Self {
        let mut buf = Vec::new();
        buf.reserve(1024);
        Self {
            buf,
            msg: String::new(),
            count_read: 0,
            count_send: 0,
            count_recv: 0,
        }
    }
}

fn client_process_read_payload<'a>(
    cache: &'a mut Cache,
    tcp: &mut TcpStream,
) -> Option<ClientResponse<'a>> {
    cache.buf.resize(1024, 0);
    if let Ok(len) = tcp.read(&mut cache.buf) {
        if len == 0 {
            return None;
        }
        Some(ClientResponse::ReadPayload {
            payload: &cache.buf[0..len],
        })
    } else {
        cache.count_read += 1;
        if cache.count_read % 3 == 0 {
            return Some(ClientResponse::Sleep {
                duration: Duration::from_millis(200),
            });
        }
        if cache.count_send < 100 {
            cache.count_send += 1;
            cache.msg = format!("packet {}", cache.count_send);
            Some(ClientResponse::Message {
                msg: cache.msg.as_ref(),
            })
        } else {
            Some(ClientResponse::ReadPayload { payload: &[] })
        }
    }
}

fn client_process_write_payload<'a>(
    tcp: &mut TcpStream,
    payload: &[u8],
) -> Option<ClientResponse<'a>> {
    tcp.write_all(payload).unwrap();
    Some(ClientResponse::ReadPayload { payload: &[] })
}

fn client_process_message<'a>(cache: &mut Cache, msg: &str) -> Option<ClientResponse<'a>> {
    dbg!(msg);
    cache.count_recv += 1;
    if cache.count_recv == 100 {
        None
    } else {
        Some(ClientResponse::ReadPayload { payload: &[] })
    }
}

fn client_process(mut tcp: TcpStream) {
    let mut cache = Cache::new();

    tcp.set_read_timeout(Some(Duration::from_millis(10)))
        .unwrap();

    let (sans, io) = asansio::new();
    let task = pin!(tlv_pingpong_proto::run_client(sans));

    let mut request = io.start(task);
    while request.is_some() {
        let response = match request.as_ref().unwrap().request() {
            Some(ClientRequest::ReadPayload) => client_process_read_payload(&mut cache, &mut tcp),
            Some(ClientRequest::WritePayload { payload }) => {
                client_process_write_payload(&mut tcp, payload)
            }
            Some(ClientRequest::Message { msg }) => client_process_message(&mut cache, msg),
            Some(ClientRequest::Error) => break,
            None => break,
        };
        let Some(response) = response else {
            break;
        };
        request = io.handle(request.take().unwrap(), &response);
    }
}
