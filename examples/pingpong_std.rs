mod pingpong_proto;
mod tlv_proto;

use asansio::Io;
use asansio::IoHandle;
use asansio::Sans;
use clap::Parser;
use std::cell::RefCell;
use std::io;
use std::io::Read;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::pin::Pin;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

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
    tcp.set_nonblocking(true).unwrap();
    println!("Connected to {}", connect);
    client_process(Client::new(tcp).unwrap());
}

enum TlvRequest {
    Send,
    Recv,
}

enum TlvResponse {
    Done,
}

fn allocate(size: usize) -> Box<[u8]> {
    let mut vec = Vec::new();
    vec.reserve_exact(size);
    vec.resize(size, 0);
    vec.into_boxed_slice()
}

struct TlvProto {
    tcp: TcpStream,

    upstream_recv: Rc<RefCell<Option<tlv_proto::Message>>>,
    upstream_send: Rc<RefCell<Option<tlv_proto::Message>>>,

    sans: Sans<TlvRequest, TlvResponse>,
}

impl tlv_proto::Iface for TlvProto {
    type Error = ();

    async fn allocate(&mut self, size: usize) -> Result<Box<[u8]>, Self::Error> {
        Ok(allocate(size))
    }

    async fn recv(&mut self, downstream_buf: &mut [u8]) -> Result<tlv_proto::Recv, Self::Error> {
        loop {
            match self.tcp.read(downstream_buf) {
                Ok(size) => return Ok(tlv_proto::Recv::Downstream(size)),
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(()),
            }

            if let Some(message) = self.upstream_recv.borrow_mut().take() {
                return Ok(tlv_proto::Recv::Upstream(message));
            }
            self.sans.handle(TlvRequest::Recv).await;
        }
    }

    async fn send_downstream(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.tcp.write(buf).map_err(|_| ())
    }

    async fn send_upstream(&mut self, message: tlv_proto::Message) -> Result<(), Self::Error> {
        loop {
            if self.upstream_send.borrow().is_none() {
                *self.upstream_send.borrow_mut() = Some(message);
                return Ok(());
            }
            self.sans.handle(TlvRequest::Send).await;
        }
    }
}

enum PpRequest {
    SendDownstream,
    SendUpstream,
    Recv,
}

enum PpResponse {
    Done,
}

struct PpProto {
    downstream_recv: Rc<RefCell<Option<tlv_proto::Message>>>,
    downstream_send: Rc<RefCell<Option<tlv_proto::Message>>>,
    upstream_recv: Rc<RefCell<Option<pingpong_proto::Message>>>,
    upstream_send: Rc<RefCell<Option<pingpong_proto::Message>>>,

    sans: Sans<PpRequest, PpResponse>,
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
        loop {
            if let Some(message) = self.downstream_recv.borrow_mut().take() {
                return Ok(pingpong_proto::Recv::Downstream(to_payload(message)));
            }
            if let Some(message) = self.upstream_recv.borrow_mut().take() {
                return Ok(pingpong_proto::Recv::Upstream(message));
            }
            self.sans.handle(PpRequest::Recv).await;
        }
    }

    async fn send_downstream(
        &mut self,
        payload: pingpong_proto::Payload,
    ) -> Result<(), Self::Error> {
        loop {
            if self.downstream_send.borrow().is_none() {
                *self.downstream_send.borrow_mut() = Some(from_payload(payload));
                return Ok(());
            }
            self.sans.handle(PpRequest::SendDownstream).await;
        }
    }

    async fn send_upstream(&mut self, message: pingpong_proto::Message) -> Result<(), Self::Error> {
        loop {
            if self.upstream_send.borrow().is_none() {
                *self.upstream_send.borrow_mut() = Some(message);
                return Ok(());
            }
            self.sans.handle(PpRequest::SendUpstream).await;
        }
    }
}

struct Client {
    tx: Rc<RefCell<Option<pingpong_proto::Message>>>,
    rx: Rc<RefCell<Option<pingpong_proto::Message>>>,

    tlv_io: Io<TlvRequest, TlvResponse>,
    tlv_handle: Option<IoHandle<TlvRequest, TlvResponse, Box<dyn Future<Output = ()>>>>,
    tlv_request: TlvRequest,

    pp_io: Io<PpRequest, PpResponse>,
    pp_handle: Option<IoHandle<PpRequest, PpResponse, Box<dyn Future<Output = ()>>>>,
    pp_request: PpRequest,
}

impl Client {
    fn new(tcp: TcpStream) -> Option<Self> {
        let (tlv_sans, tlv_io) = asansio::new::<TlvRequest, TlvResponse>();
        let (pp_sans, pp_io) = asansio::new::<PpRequest, PpResponse>();
        let tlv_to_pp = Rc::new(RefCell::new(None));
        let pp_to_tlv = Rc::new(RefCell::new(None));
        let pp_to_main = Rc::new(RefCell::new(None));
        let main_to_pp = Rc::new(RefCell::new(None));

        let tlv_proto = Box::pin({
            let pp_to_tlv = Rc::clone(&pp_to_tlv);
            let tlv_to_pp = Rc::clone(&tlv_to_pp);
            async move {
                tlv_proto::run(TlvProto {
                    tcp,
                    upstream_recv: pp_to_tlv,
                    upstream_send: tlv_to_pp,
                    sans: tlv_sans,
                })
                .await
                .unwrap_or(());
            }
        }) as Pin<Box<dyn Future<Output = ()>>>;
        let pp_proto = Box::pin({
            let main_to_pp = Rc::clone(&main_to_pp);
            let pp_to_main = Rc::clone(&pp_to_main);
            async move {
                pingpong_proto::run(PpProto {
                    downstream_recv: tlv_to_pp,
                    downstream_send: pp_to_tlv,
                    upstream_recv: main_to_pp,
                    upstream_send: pp_to_main,
                    sans: pp_sans,
                })
                .await
                .unwrap_or(());
            }
        }) as Pin<Box<dyn Future<Output = ()>>>;

        let (tlv_handle, tlv_request) = tlv_io.start(tlv_proto)?;
        let tlv_handle = Some(tlv_handle);
        let (pp_handle, pp_request) = pp_io.start(pp_proto)?;
        let pp_handle = Some(pp_handle);

        Some(Self {
            tx: main_to_pp,
            rx: pp_to_main,
            tlv_io,
            tlv_handle,
            tlv_request,
            pp_io,
            pp_handle,
            pp_request,
        })
    }

    fn send_message(&mut self, msg: String) -> Option<pingpong_proto::Message> {
        assert!(self.tx.borrow().is_none());
        *self.tx.borrow_mut() = Some(pingpong_proto::Message::new_message(msg)?);
        self.process_send()?;
        self.process_recv()?;
        self.rx.borrow_mut().take()
    }

    fn send_sleep(&mut self, duration: Duration) -> Option<()> {
        assert!(self.tx.borrow().is_none());
        *self.tx.borrow_mut() = Some(pingpong_proto::Message::Sleep(duration));
        self.process_send()
    }

    fn process_send(&mut self) -> Option<()> {
        while self.tx.borrow().is_some() || !matches!(self.tlv_request, TlvRequest::Recv) {
            self.process()?;
        }
        Some(())
    }

    fn process_recv(&mut self) -> Option<()> {
        while self.rx.borrow().is_none() {
            self.process()?;
        }
        Some(())
    }

    fn process(&mut self) -> Option<()> {
        let (tlv_handle, tlv_request) = self
            .tlv_io
            .handle(self.tlv_handle.take()?, TlvResponse::Done)?;
        self.tlv_handle = Some(tlv_handle);
        self.tlv_request = tlv_request;
        let (pp_handle, pp_request) = self
            .pp_io
            .handle(self.pp_handle.take()?, PpResponse::Done)?;
        self.pp_handle = Some(pp_handle);
        self.pp_request = pp_request;
        Some(())
    }
}

fn client_process(mut client: Client) {
    for count in 1..=100usize {
        let Some(msg) = client.send_message(format!("count {count}")) else {
            return;
        };
        match msg {
            pingpong_proto::Message::Message(msg) => {
                println!("msg: {msg}");
            }
            pingpong_proto::Message::Sleep(duration) => println!("duration: {duration:?}"),
        }
        if count.is_multiple_of(3) && client.send_sleep(Duration::from_millis(200)).is_none() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}
