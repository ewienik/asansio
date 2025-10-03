use core::pin::pin;

#[test]
fn no_response() {
    struct Request;
    struct Response;

    let (_, io) = sansio::new::<Request, Response>();

    let task = pin!(async {});
    assert!(io.start(task).is_none());
}

#[test]
fn single_call() {
    struct Request;
    struct Response;

    let (sans, io) = sansio::new::<Request, Response>();

    let task = pin!(async {
        let response = sans.start(&Request).await;
        assert!(matches!(response.response(), Some(&Response)));
    });

    let request = io.start(task).unwrap();
    assert!(matches!(request.request(), Some(&Request)));

    assert!(io.handle(request, &Response).is_none());
}

#[test]
fn send_owned_payload() {
    struct Request([u8; 10]);
    struct Response([u8; 20]);

    let (sans, io) = sansio::new::<Request, Response>();

    let task = pin!(async {
        let response = sans.start(&Request([1; 10])).await;
        assert!(matches!(response.response(), Some(&Response(_))));
        assert_eq!(response.response().unwrap().0, [2; 20]);

        let response = sans.handle(response, &Request([3; 10])).await;
        assert!(matches!(response.response(), Some(&Response(_))));
        assert_eq!(response.response().unwrap().0, [4; 20]);
    });

    let request = io.start(task).unwrap();
    assert!(matches!(request.request(), Some(&Request(_))));
    assert_eq!(request.request().unwrap().0, [1; 10]);

    let request = io.handle(request, &Response([2; 20])).unwrap();
    assert!(matches!(request.request(), Some(&Request(_))));
    assert_eq!(request.request().unwrap().0, [3; 10]);

    assert!(io.handle(request, &Response([4; 20])).is_none());
}

#[test]
fn send_borrowed_payload() {
    struct Request<'a>(&'a [u8]);
    struct Response<'a>(&'a [u8]);

    let (sans, io) = sansio::new::<Request, Response>();

    let task = pin!(async {
        let mut request_buf = vec![0u8; 10];

        request_buf.fill(1);
        let response = sans.start(&Request(&request_buf)).await;
        assert!(matches!(response.response(), Some(&Response(_))));
        assert_eq!(response.response().unwrap().0, [2; 20]);

        request_buf.fill(3);
        let response = sans.handle(response, &Request(&request_buf)).await;
        assert!(matches!(response.response(), Some(&Response(_))));
        assert_eq!(response.response().unwrap().0, [4; 20]);

        drop(request_buf);
        let mut request_buf = vec![0u8; 10];

        request_buf.fill(5);
        let response = sans.handle(response, &Request(&request_buf)).await;
        assert!(matches!(response.response(), Some(&Response(_))));
        assert_eq!(response.response().unwrap().0, [6; 20]);
    });

    let request = io.start(task).unwrap();
    assert!(matches!(request.request(), Some(&Request(_))));
    assert_eq!(request.request().unwrap().0, [1; 10]);

    let mut response_buf = vec![0; 20];

    response_buf.fill(2);
    let request = io.handle(request, &Response(&response_buf)).unwrap();
    assert!(matches!(request.request(), Some(&Request(_))));
    assert_eq!(request.request().unwrap().0, [3; 10]);

    response_buf.fill(4);
    let request = io.handle(request, &Response(&response_buf)).unwrap();
    assert!(matches!(request.request(), Some(&Request(_))));
    assert_eq!(request.request().unwrap().0, [5; 10]);

    drop(response_buf);
    let mut response_buf = vec![0; 20];

    response_buf.fill(6);
    assert!(io.handle(request, &Response(&response_buf)).is_none());
}
