use asansio::Sans;
use core::cell::RefCell;
use core::pin::pin;
use std::rc::Rc;
use tokio::sync::mpsc;

#[test]
fn no_response() {
    struct Request;
    struct Response;

    let (_, io) = asansio::new::<Request, Response>();

    let task = pin!(async {});
    assert!(io.start(task).is_none());
}

#[test]
fn single_call() {
    struct Request;
    struct Response;

    let (sans, io) = asansio::new::<Request, Response>();

    let task = pin!(async {
        let handle = sans.handle(Request).await;
        assert!(matches!(handle.message(), Some(&Response)));
    });

    let (handle, request) = io.start(task).unwrap();
    assert!(matches!(request, Request));

    assert!(io.handle(handle, Response).is_none());
}

#[test]
fn pin_box_single_call() {
    struct Request;
    struct Response;

    let (sans, io) = asansio::new::<Request, Response>();

    let task = Box::pin(async {
        let handle = sans.handle(Request).await;
        assert!(matches!(handle.message(), Some(&Response)));
    });

    let (handle, request) = io.start(task).unwrap();
    assert!(matches!(request, Request));

    assert!(io.handle(handle, Response).is_none());
}

#[test]
fn send_owned_payload() {
    struct Request([u8; 10]);
    struct Response([u8; 20]);

    let (sans, io) = asansio::new::<Request, Response>();

    let task = pin!(async {
        let handle = sans.handle(Request([1; 10])).await;
        assert!(matches!(handle.message(), Some(Response(_))));
        assert_eq!(handle.message().unwrap().0, [2; 20]);

        let handle = sans.handle(Request([3; 10])).await;
        assert!(matches!(handle.message(), Some(Response(_))));
        assert_eq!(handle.message().unwrap().0, [4; 20]);
    });

    let (handle, request) = io.start(task).unwrap();
    assert!(matches!(request, Request(_)));
    assert_eq!(request.0, [1; 10]);

    let (handle, request) = io.handle(handle, Response([2; 20])).unwrap();
    assert!(matches!(request, Request(_)));
    assert_eq!(request.0, [3; 10]);

    assert!(io.handle(handle, Response([4; 20])).is_none());
}

trait Protocol {
    async fn alloc(&mut self, size: usize) -> Box<[u8]>;
    async fn send(&mut self, buf: &[u8]) -> usize;
    async fn recv(&mut self, buf: &mut [u8]) -> usize;
}

enum ProtocolRequest {
    Alloc,
    Send,
    Recv,
}

enum ProtocolResponse {
    Wait,
    Done,
}

struct ProtocolSync {
    buffer: Rc<RefCell<Vec<u8>>>,
    sans: Sans<ProtocolRequest, ProtocolResponse>,
}

impl ProtocolSync {
    fn new(buffer: Rc<RefCell<Vec<u8>>>, sans: Sans<ProtocolRequest, ProtocolResponse>) -> Self {
        Self { buffer, sans }
    }
}

impl Protocol for ProtocolSync {
    async fn alloc(&mut self, size: usize) -> Box<[u8]> {
        loop {
            let handle = self.sans.handle(ProtocolRequest::Alloc).await;
            if matches!(handle.message(), Some(ProtocolResponse::Done)) {
                break;
            }
        }
        vec![0; size].into_boxed_slice()
    }

    async fn send(&mut self, buf: &[u8]) -> usize {
        {
            let mut buffer = self.buffer.borrow_mut();
            buffer.clear();
            buffer.extend_from_slice(buf);
        }
        loop {
            let handle = self.sans.handle(ProtocolRequest::Send).await;
            if matches!(handle.message(), Some(ProtocolResponse::Done)) {
                break;
            }
        }
        self.buffer.borrow().len()
    }

    async fn recv(&mut self, buf: &mut [u8]) -> usize {
        self.buffer.borrow_mut().clear();
        loop {
            let handle = self.sans.handle(ProtocolRequest::Recv).await;
            if matches!(handle.message(), Some(ProtocolResponse::Done)) {
                break;
            }
        }
        buf.iter_mut()
            .zip(self.buffer.borrow().iter())
            .map(|(dst, src)| *dst = *src)
            .count()
    }
}

struct ProtocolTokio {
    buffer: Vec<u8>,
    tx: mpsc::Sender<String>,
}

impl ProtocolTokio {
    fn new(tx: mpsc::Sender<String>) -> Self {
        Self { buffer: vec![], tx }
    }
}

impl Protocol for ProtocolTokio {
    async fn alloc(&mut self, size: usize) -> Box<[u8]> {
        self.tx.send(format!("alloc({size})")).await.unwrap();
        vec![0; size].into_boxed_slice()
    }

    async fn send(&mut self, buf: &[u8]) -> usize {
        self.buffer.clear();
        self.buffer.extend_from_slice(buf);
        self.tx.send(format!("send({buf:?})")).await.unwrap();
        self.buffer.len()
    }

    async fn recv(&mut self, buf: &mut [u8]) -> usize {
        self.buffer.iter_mut().for_each(|v| *v += 1);
        self.tx
            .send(format!("recv({:?})", self.buffer))
            .await
            .unwrap();
        buf.iter_mut()
            .zip(self.buffer.iter())
            .map(|(dst, src)| *dst = *src)
            .count()
    }
}

async fn run(mut proto: impl Protocol) {
    let mut buffer = proto.alloc(2).await;

    buffer.fill(b'a');
    assert_eq!(proto.send(&buffer).await, 2);

    assert_eq!(proto.recv(&mut buffer).await, 2);
    assert_eq!(buffer.iter().as_slice(), *b"bb");

    buffer.fill(b'c');
    assert_eq!(proto.send(&buffer).await, 2);

    assert_eq!(proto.recv(&mut buffer).await, 2);
    assert_eq!(buffer.iter().as_slice(), *b"dd");
}

#[test]
fn simple_protocol_sync() {
    let (sans, io) = asansio::new::<ProtocolRequest, ProtocolResponse>();
    let buffer = Rc::new(RefCell::new(vec![]));
    let proto = ProtocolSync::new(Rc::clone(&buffer), sans);

    let task = pin!(run(proto));

    let (handle, request) = io.start(task).unwrap();
    assert!(matches!(request, ProtocolRequest::Alloc));

    let (handle, request) = io.handle(handle, ProtocolResponse::Wait).unwrap();
    assert!(matches!(request, ProtocolRequest::Alloc));

    let (handle, request) = io.handle(handle, ProtocolResponse::Done).unwrap();
    assert!(matches!(request, ProtocolRequest::Send));
    assert_eq!(buffer.borrow().as_slice(), *b"aa");

    let (handle, request) = io.handle(handle, ProtocolResponse::Wait).unwrap();
    assert!(matches!(request, ProtocolRequest::Send));
    assert_eq!(buffer.borrow().as_slice(), *b"aa");

    let (handle, request) = io.handle(handle, ProtocolResponse::Done).unwrap();
    assert!(matches!(request, ProtocolRequest::Recv));

    let (handle, request) = io.handle(handle, ProtocolResponse::Wait).unwrap();
    assert!(matches!(request, ProtocolRequest::Recv));

    buffer.borrow_mut().clear();
    buffer.borrow_mut().extend_from_slice(b"bb");
    let (handle, request) = io.handle(handle, ProtocolResponse::Done).unwrap();
    assert!(matches!(request, ProtocolRequest::Send));
    assert_eq!(buffer.borrow().as_slice(), *b"cc");

    let (handle, request) = io.handle(handle, ProtocolResponse::Done).unwrap();
    assert!(matches!(request, ProtocolRequest::Recv));

    buffer.borrow_mut().clear();
    buffer.borrow_mut().extend_from_slice(b"dd");
    assert!(io.handle(handle, ProtocolResponse::Done).is_none());
}

#[tokio::test]
async fn simple_protocol_tokio() {
    let (tx, mut rx) = mpsc::channel(1);
    let task = tokio::spawn(run(ProtocolTokio::new(tx)));

    assert_eq!(rx.recv().await, Some("alloc(2)".into()));
    assert_eq!(rx.recv().await, Some("send([97, 97])".into()));
    assert_eq!(rx.recv().await, Some("recv([98, 98])".into()));
    assert_eq!(rx.recv().await, Some("send([99, 99])".into()));
    assert_eq!(rx.recv().await, Some("recv([100, 100])".into()));

    task.await.unwrap();
}
