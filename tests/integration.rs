use asansio::Sans;
use asansio::SansHandle;
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
        let handle = sans.start(&Request).await;
        assert!(matches!(handle.message(), Some(&Response)));
    });

    let handle = io.start(task).unwrap();
    assert!(matches!(handle.message(), Some(&Request)));

    assert!(io.handle(handle, &Response).is_none());
}

#[test]
fn send_owned_payload() {
    struct Request([u8; 10]);
    struct Response([u8; 20]);

    let (sans, io) = asansio::new::<Request, Response>();

    let task = pin!(async {
        let handle = sans.start(&Request([1; 10])).await;
        assert!(matches!(handle.message(), Some(&Response(_))));
        assert_eq!(handle.message().unwrap().0, [2; 20]);

        let handle = sans.handle(handle, &Request([3; 10])).await;
        assert!(matches!(handle.message(), Some(&Response(_))));
        assert_eq!(handle.message().unwrap().0, [4; 20]);
    });

    let handle = io.start(task).unwrap();
    assert!(matches!(handle.message(), Some(&Request(_))));
    assert_eq!(handle.message().unwrap().0, [1; 10]);

    let handle = io.handle(handle, &Response([2; 20])).unwrap();
    assert!(matches!(handle.message(), Some(&Request(_))));
    assert_eq!(handle.message().unwrap().0, [3; 10]);

    assert!(io.handle(handle, &Response([4; 20])).is_none());
}

#[test]
fn send_borrowed_payload() {
    struct Request<'a>(&'a [u8]);
    struct Response<'a>(&'a [u8]);

    let (sans, io) = asansio::new::<Request, Response>();

    let task = pin!(async {
        let mut request_buf = vec![0u8; 10];

        request_buf.fill(1);
        let handle = sans.start(&Request(&request_buf)).await;
        assert!(matches!(handle.message(), Some(&Response(_))));
        assert_eq!(handle.message().unwrap().0, [2; 20]);

        request_buf.fill(3);
        let handle = sans.handle(handle, &Request(&request_buf)).await;
        assert!(matches!(handle.message(), Some(&Response(_))));
        assert_eq!(handle.message().unwrap().0, [4; 20]);

        drop(request_buf);
        let mut request_buf = vec![0u8; 10];

        request_buf.fill(5);
        let handle = sans.handle(handle, &Request(&request_buf)).await;
        assert!(matches!(handle.message(), Some(&Response(_))));
        assert_eq!(handle.message().unwrap().0, [6; 20]);
    });

    let handle = io.start(task).unwrap();
    assert!(matches!(handle.message(), Some(&Request(_))));
    assert_eq!(handle.message().unwrap().0, [1; 10]);

    let mut response_buf = vec![0; 20];

    response_buf.fill(2);
    let handle = io.handle(handle, &Response(&response_buf)).unwrap();
    assert!(matches!(handle.message(), Some(&Request(_))));
    assert_eq!(handle.message().unwrap().0, [3; 10]);

    response_buf.fill(4);
    let handle = io.handle(handle, &Response(&response_buf)).unwrap();
    assert!(matches!(handle.message(), Some(&Request(_))));
    assert_eq!(handle.message().unwrap().0, [5; 10]);

    drop(response_buf);
    let mut response_buf = vec![0; 20];

    response_buf.fill(6);
    assert!(io.handle(handle, &Response(&response_buf)).is_none());
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
    handle: Option<SansHandle<ProtocolResponse>>,
}

impl ProtocolSync {
    fn new(buffer: Rc<RefCell<Vec<u8>>>, sans: Sans<ProtocolRequest, ProtocolResponse>) -> Self {
        Self {
            buffer,
            sans,
            handle: None,
        }
    }
}

impl Protocol for ProtocolSync {
    async fn alloc(&mut self, size: usize) -> Box<[u8]> {
        loop {
            let handle = if let Some(handle) = self.handle.take() {
                self.sans.handle(handle, &ProtocolRequest::Alloc).await
            } else {
                self.sans.start(&ProtocolRequest::Alloc).await
            };
            self.handle.replace(handle);
            if matches!(
                self.handle.as_ref().unwrap().message(),
                Some(ProtocolResponse::Done)
            ) {
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
            let handle = if let Some(handle) = self.handle.take() {
                self.sans.handle(handle, &ProtocolRequest::Send).await
            } else {
                self.sans.start(&ProtocolRequest::Send).await
            };
            self.handle.replace(handle);
            if matches!(
                self.handle.as_ref().unwrap().message(),
                Some(ProtocolResponse::Done)
            ) {
                break;
            }
        }
        self.buffer.borrow().len()
    }

    async fn recv(&mut self, buf: &mut [u8]) -> usize {
        self.buffer.borrow_mut().clear();
        loop {
            let handle = if let Some(handle) = self.handle.take() {
                self.sans.handle(handle, &ProtocolRequest::Recv).await
            } else {
                self.sans.start(&ProtocolRequest::Recv).await
            };
            self.handle.replace(handle);
            if matches!(
                self.handle.as_ref().unwrap().message(),
                Some(ProtocolResponse::Done)
            ) {
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
    assert_eq!(buffer.iter().as_slice(), [b'b', b'b']);

    buffer.fill(b'c');
    assert_eq!(proto.send(&buffer).await, 2);

    assert_eq!(proto.recv(&mut buffer).await, 2);
    assert_eq!(buffer.iter().as_slice(), [b'd', b'd']);
}

#[test]
fn simple_protocol_sync() {
    let (sans, io) = asansio::new::<ProtocolRequest, ProtocolResponse>();
    let buffer = Rc::new(RefCell::new(vec![]));
    let proto = ProtocolSync::new(Rc::clone(&buffer), sans);

    let task = pin!(run(proto));

    let handle = io.start(task).unwrap();
    assert!(matches!(handle.message(), Some(&ProtocolRequest::Alloc)));

    let handle = io.handle(handle, &ProtocolResponse::Wait).unwrap();
    assert!(matches!(handle.message(), Some(&ProtocolRequest::Alloc)));

    let handle = io.handle(handle, &ProtocolResponse::Done).unwrap();
    assert!(matches!(handle.message(), Some(&ProtocolRequest::Send)));
    assert_eq!(buffer.borrow().as_slice(), [b'a', b'a']);

    let handle = io.handle(handle, &ProtocolResponse::Wait).unwrap();
    assert!(matches!(handle.message(), Some(&ProtocolRequest::Send)));
    assert_eq!(buffer.borrow().as_slice(), [b'a', b'a']);

    let handle = io.handle(handle, &ProtocolResponse::Done).unwrap();
    assert!(matches!(handle.message(), Some(&ProtocolRequest::Recv)));

    let handle = io.handle(handle, &ProtocolResponse::Wait).unwrap();
    assert!(matches!(handle.message(), Some(&ProtocolRequest::Recv)));

    buffer.borrow_mut().clear();
    buffer.borrow_mut().extend_from_slice(&[b'b', b'b']);
    let handle = io.handle(handle, &ProtocolResponse::Done).unwrap();
    assert!(matches!(handle.message(), Some(&ProtocolRequest::Send)));
    assert_eq!(buffer.borrow().as_slice(), [b'c', b'c']);

    let handle = io.handle(handle, &ProtocolResponse::Done).unwrap();
    assert!(matches!(handle.message(), Some(&ProtocolRequest::Recv)));

    buffer.borrow_mut().clear();
    buffer.borrow_mut().extend_from_slice(&[b'd', b'd']);
    assert!(io.handle(handle, &ProtocolResponse::Done).is_none());
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
