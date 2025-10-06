mod tlv_pingpong_proto;

use clap::Parser;
use clap::Subcommand;
use std::net::SocketAddr;
use std::pin::pin;
use std::time::Duration;
use tlv_pingpong_proto::ClientRequest;
use tlv_pingpong_proto::ClientResponse;
use tlv_pingpong_proto::ServerRequest;
use tlv_pingpong_proto::ServerResponse;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::Interest;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::time;

#[derive(Parser)]
#[clap(version)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Client {
        #[clap(short, long, default_value = "127.0.0.1:7123")]
        connect: SocketAddr,
    },
    Server {
        #[clap(short, long, default_value = "127.0.0.1:7123")]
        listen: SocketAddr,
    },
}

#[tokio::main]
async fn main() {
    match Args::parse().command {
        Command::Client { connect } => client(connect).await,
        Command::Server { listen } => server(listen).await,
    }
}

async fn client(connect: SocketAddr) {
    let tcp = TcpStream::connect(connect).await.unwrap();
    println!("Connected to {}", connect);
    client_process(tcp).await;
}

async fn server(listen: SocketAddr) {
    let listener = TcpListener::bind(listen).await.unwrap();
    println!("Listening at {}", listener.local_addr().unwrap());

    while let Ok((tcp, addr)) = listener.accept().await {
        println!("Accepted peer from {}", addr);
        tokio::spawn(server_process(tcp));
    }
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

async fn is_eof(tcp: &TcpStream) -> bool {
    tcp.ready(Interest::READABLE)
        .await
        .unwrap()
        .is_read_closed()
}

async fn client_process_read_payload<'a>(
    cache: &'a mut Cache,
    tcp: &mut TcpStream,
) -> Option<ClientResponse<'a>> {
    cache.buf.resize(1024, 0);
    if let Ok(Ok(len)) = time::timeout(Duration::from_millis(10), tcp.read(&mut cache.buf)).await {
        if len == 0 && is_eof(tcp).await {
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

async fn client_process_write_payload<'a>(
    tcp: &mut TcpStream,
    payload: &[u8],
) -> Option<ClientResponse<'a>> {
    tcp.write_all(payload).await.unwrap();
    Some(ClientResponse::ReadPayload { payload: &[] })
}

async fn client_process_message<'a>(cache: &mut Cache, msg: &str) -> Option<ClientResponse<'a>> {
    dbg!(msg);
    cache.count_recv += 1;
    if cache.count_recv == 100 {
        None
    } else {
        Some(ClientResponse::ReadPayload { payload: &[] })
    }
}

async fn client_process(mut tcp: TcpStream) {
    let mut cache = Cache::new();

    let (sans, io) = asansio::new();
    let task = pin!(tlv_pingpong_proto::run_client(sans));

    let mut request = io.start(task);
    while request.is_some() {
        let response = match request.as_ref().unwrap().request() {
            Some(ClientRequest::ReadPayload) => {
                client_process_read_payload(&mut cache, &mut tcp).await
            }
            Some(ClientRequest::WritePayload { payload }) => {
                client_process_write_payload(&mut tcp, payload).await
            }
            Some(ClientRequest::Message { msg }) => client_process_message(&mut cache, msg).await,
            Some(ClientRequest::Error) => break,
            None => break,
        };
        let Some(response) = response else {
            break;
        };
        request = io.handle(request.take().unwrap(), &response);
    }
}

async fn server_process_read_payload<'a>(
    cache: &'a mut Cache,
    tcp: &mut TcpStream,
) -> Option<ServerResponse<'a>> {
    cache.buf.resize(1024, 0);
    let Ok(len) = tcp.read(&mut cache.buf).await else {
        return None;
    };
    if len == 0 && is_eof(tcp).await {
        None
    } else {
        Some(ServerResponse::ReadPayload {
            payload: &cache.buf[0..len],
        })
    }
}

async fn server_process_write_payload<'a>(
    tcp: &mut TcpStream,
    payload: &[u8],
) -> Option<ServerResponse<'a>> {
    tcp.write_all(payload).await.unwrap();
    Some(ServerResponse::ReadPayload { payload: &[] })
}

async fn server_process_sleep<'a>(duration: Duration) -> Option<ServerResponse<'a>> {
    time::sleep(duration).await;
    Some(ServerResponse::ReadPayload { payload: &[] })
}

async fn server_process(mut tcp: TcpStream) {
    let mut cache = Cache::new();

    let (sans, io) = asansio::new();
    let task = pin!(tlv_pingpong_proto::run_server(sans));

    let mut request = io.start(task);
    while request.is_some() {
        let response = match request.as_ref().unwrap().request() {
            Some(ServerRequest::ReadPayload) => {
                server_process_read_payload(&mut cache, &mut tcp).await
            }
            Some(ServerRequest::WritePayload { payload }) => {
                server_process_write_payload(&mut tcp, payload).await
            }
            Some(ServerRequest::Sleep { duration }) => server_process_sleep(*duration).await,
            Some(ServerRequest::Error) => break,
            None => break,
        };
        let Some(response) = response else {
            break;
        };
        request = io.handle(request.take().unwrap(), &response);
    }
}
