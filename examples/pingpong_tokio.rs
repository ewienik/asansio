mod pingpong_proto;
mod tlv_proto;

use clap::Parser;
use clap::Subcommand;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::sync::mpsc;
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
    client_process(Client::new(tcp)).await;
}

async fn server(listen: SocketAddr) {
    let listener = TcpListener::bind(listen).await.unwrap();
    println!("Listening at {}", listener.local_addr().unwrap());

    while let Ok((tcp, addr)) = listener.accept().await {
        println!("Accepted peer from {}", addr);
        tokio::spawn(server_process(tcp));
    }
}

fn allocate(size: usize) -> Box<[u8]> {
    let mut vec = Vec::new();
    vec.reserve_exact(size);
    vec.resize(size, 0);
    vec.into_boxed_slice()
}

struct TlvProto {
    tcp: TcpStream,
    upstream_send: mpsc::Sender<tlv_proto::Message>,
    upstream_recv: mpsc::Receiver<tlv_proto::Message>,
}

impl tlv_proto::Iface for TlvProto {
    type Error = ();

    async fn allocate(&mut self, size: usize) -> Result<Box<[u8]>, Self::Error> {
        Ok(allocate(size))
    }

    async fn recv(&mut self, downstream_buf: &mut [u8]) -> Result<tlv_proto::Recv, Self::Error> {
        tokio::select! {
            read = self.tcp.read(downstream_buf) => {
                if let Ok(len) = read && len > 0 {
                    Ok(tlv_proto::Recv::Downstream(len))
                }
                else {
                    Err(())
                }

            }

            recv = self.upstream_recv.recv() => {
                if let Some(message) = recv {
                    Ok(tlv_proto::Recv::Upstream(message))
                } else {
                    Err(())
                }
            }
        }
    }

    async fn send_downstream(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.tcp.write(buf).await.map_err(|_| ())
    }

    async fn send_upstream(&mut self, message: tlv_proto::Message) -> Result<(), Self::Error> {
        self.upstream_send.send(message).await.map_err(|_| ())
    }
}

struct PpProto {
    downstream_send: mpsc::Sender<tlv_proto::Message>,
    downstream_recv: mpsc::Receiver<tlv_proto::Message>,
    upstream_send: mpsc::Sender<pingpong_proto::Message>,
    upstream_recv: mpsc::Receiver<pingpong_proto::Message>,
}

fn to_payload(message: tlv_proto::Message) -> pingpong_proto::Payload {
    match message.tag {
        0 => pingpong_proto::Payload::Message(message.buf),
        1 => pingpong_proto::Payload::Sleep(message.buf),
        _ => unimplemented!(),
    }
}

fn from_payload(payload: pingpong_proto::Payload) -> tlv_proto::Message {
    match payload {
        pingpong_proto::Payload::Message(buf) => tlv_proto::Message::new(0, buf).unwrap(),
        pingpong_proto::Payload::Sleep(buf) => tlv_proto::Message::new(1, buf).unwrap(),
    }
}

impl pingpong_proto::Iface for PpProto {
    type Error = ();

    async fn allocate(&mut self, size: usize) -> Result<Box<[u8]>, Self::Error> {
        Ok(allocate(size))
    }

    async fn recv(&mut self) -> Result<pingpong_proto::Recv, Self::Error> {
        tokio::select! {
            recv = self.downstream_recv.recv() => {
                if let Some(message) = recv {
                    Ok(pingpong_proto::Recv::Downstream(to_payload(message)))
                }
                else {
                    Err(())
                }

            }

            recv = self.upstream_recv.recv() => {
                if let Some(message) = recv {
                    Ok(pingpong_proto::Recv::Upstream(message))
                } else {
                    Err(())
                }
            }
        }
    }

    async fn send_downstream(
        &mut self,
        payload: pingpong_proto::Payload,
    ) -> Result<(), Self::Error> {
        self.downstream_send
            .send(from_payload(payload))
            .await
            .map_err(|_| ())
    }

    async fn send_upstream(&mut self, message: pingpong_proto::Message) -> Result<(), Self::Error> {
        self.upstream_send.send(message).await.map_err(|_| ())
    }
}

fn process(
    tcp: TcpStream,
) -> (
    mpsc::Sender<pingpong_proto::Message>,
    mpsc::Receiver<pingpong_proto::Message>,
    Arc<Notify>,
) {
    let (tx_tlv_to_pp, rx_tlv_to_pp) = mpsc::channel(10);
    let (tx_pp_to_tlv, rx_pp_to_tlv) = mpsc::channel(10);
    let (tx_pp_to_main, rx_pp_to_main) = mpsc::channel(10);
    let (tx_main_to_pp, rx_main_to_pp) = mpsc::channel(10);

    let notify = Arc::new(Notify::new());
    tokio::spawn({
        let notify = Arc::clone(&notify);
        async move {
            _ = tlv_proto::run(TlvProto {
                tcp,
                upstream_recv: rx_pp_to_tlv,
                upstream_send: tx_tlv_to_pp,
            })
            .await;
            notify.notify_one();
        }
    });

    tokio::spawn({
        let notify = Arc::clone(&notify);
        async move {
            _ = pingpong_proto::run(PpProto {
                downstream_recv: rx_tlv_to_pp,
                downstream_send: tx_pp_to_tlv,
                upstream_recv: rx_main_to_pp,
                upstream_send: tx_pp_to_main,
            })
            .await;
            notify.notify_one();
        }
    });
    (tx_main_to_pp, rx_pp_to_main, notify)
}

struct Client {
    tx: mpsc::Sender<pingpong_proto::Message>,
    rx: mpsc::Receiver<pingpong_proto::Message>,
}

impl Client {
    fn new(tcp: TcpStream) -> Self {
        let (tx, rx, _) = process(tcp);
        Self { tx, rx }
    }

    async fn send_message(&mut self, msg: String) -> Option<pingpong_proto::Message> {
        self.tx
            .send(pingpong_proto::Message::new_message(msg)?)
            .await
            .ok()?;
        self.rx.recv().await
    }

    async fn send_sleep(&self, duration: Duration) -> bool {
        self.tx
            .send(pingpong_proto::Message::Sleep(duration))
            .await
            .is_ok()
    }
}

async fn client_process(mut client: Client) {
    let mut interval = time::interval(Duration::from_millis(10));
    let mut count = 0usize;
    loop {
        interval.tick().await;
        count += 1;
        if count.is_multiple_of(3) && !client.send_sleep(Duration::from_millis(200)).await {
            break;
        }
        if count > 100 {
            break;
        }
        match client.send_message(format!("count {count}")).await {
            Some(pingpong_proto::Message::Message(msg)) => {
                println!("msg: {msg}");
            }
            Some(pingpong_proto::Message::Sleep(duration)) => println!("duration: {duration:?}"),
            None => break,
        }
    }
}

async fn server_process(tcp: TcpStream) {
    let (tx, mut rx, notify) = process(tcp);

    loop {
        tokio::select! {
            recv = rx.recv() => {
                let Some(msg) = recv else {
                    break;
                };
                match &msg {
                    pingpong_proto::Message::Message(_) => tx.send(msg).await.unwrap(),
                    pingpong_proto::Message::Sleep(duration) => time::sleep(*duration).await,
                }
            }

            _ = notify.notified() => {
                break;
            }
        }
    }
}
