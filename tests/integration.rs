use core::pin::pin;

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
