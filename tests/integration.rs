use core::pin::pin;
use sansio::SansIo;

#[test]
fn no_response() {
    struct Request;
    struct Response;

    let task = pin!(async {});
    let (_, request): (SansIo<Request, Response, _>, _) = SansIo::start(task);
    assert!(matches!(request, None));
}

#[test]
fn single_call() {
    struct Request;
    struct Response;

    let task = pin!(async {
        let response = sansio::call(Request).await;
        assert!(matches!(response, Response));
    });

    let (mut sansio, request) = SansIo::start(task);
    assert!(matches!(request, Some(Request)));

    let request = sansio.handle(Response);
    assert!(matches!(request, None));
}

#[test]
fn send_owned_payload() {
    struct Request([u8; 10]);
    struct Response([u8; 20]);

    let task = pin!(async {
        let response = sansio::call(Request([1; 10])).await;
        assert!(matches!(response, Response(_)));
        assert_eq!(response.0, [2; 20]);

        let response = sansio::call(Request([3; 10])).await;
        assert!(matches!(response, Response(_)));
        assert_eq!(response.0, [4; 20]);
    });

    let (mut sansio, request) = SansIo::start(task);
    assert!(matches!(request, Some(Request(_))));
    assert_eq!(request.unwrap().0, [1; 10]);

    let request = sansio.handle(Response([2; 20]));
    assert!(matches!(request, Some(Request(_))));
    assert_eq!(request.unwrap().0, [3; 10]);

    let request = sansio.handle(Response([4; 20]));
    assert!(matches!(request, None));
}

#[test]
fn send_borrowed_payload() {
    struct Request<'a>(&'a [u8]);
    struct Response<'a>(&'a [u8]);

    let task = pin!(async {
        let mut request_buf = [0u8; 10];
        request_buf.fill(1);
        let response = sansio::call(Request(&request_buf)).await;
        assert!(matches!(response, Response(_)));
        assert_eq!(response.0, &[2; 20]);

        request_buf.fill(3);
        let response = sansio::call(Request(&request_buf)).await;
        assert!(matches!(response, Response(_)));
        assert_eq!(response.0, &[4; 20]);
    });

    let (mut sansio, request) = SansIo::start(task);
    assert!(matches!(request, Some(Request(_))));
    assert_eq!(request.unwrap().0, &[1; 10]);

    let response_buf = [2; 20];
    let request = sansio.handle(Response(&response_buf));
    assert!(matches!(request, Some(Request(_))));
    assert_eq!(request.unwrap().0, &[3; 10]);

    let response_buf = [4; 20];
    let request = sansio.handle(Response(&response_buf));
    assert!(matches!(request, None));
}
