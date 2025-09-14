use core::pin::pin;
use sansio::SansIo;

#[test]
fn no_response() {
    struct Request;
    struct Response;

    let task = pin!(async {});
    assert!(SansIo::<Request, Response, _>::start(task).is_none());
}

#[test]
fn single_call() {
    struct Request;
    struct Response;

    let task = pin!(async {
        let response = sansio::call(Request).await;
        assert!(matches!(response, Response));
    });

    let (sansio, request) = SansIo::start(task).unwrap();
    assert!(matches!(request, Request));

    assert!(sansio.handle(Response).is_none());
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

    let (sansio, request) = SansIo::start(task).unwrap();
    assert!(matches!(request, Request(_)));
    assert_eq!(request.0, [1; 10]);

    let (sansio, request) = sansio.handle(Response([2; 20])).unwrap();
    assert!(matches!(request, Request(_)));
    assert_eq!(request.0, [3; 10]);

    assert!(sansio.handle(Response([4; 20])).is_none());
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

    let (sansio, request) = SansIo::start(task).unwrap();
    assert!(matches!(request, Request(_)));
    assert_eq!(request.0, &[1; 10]);

    let response_buf = [2; 20];
    let (sansio, request) = sansio.handle(Response(&response_buf)).unwrap();
    assert!(matches!(request, Request(_)));
    assert_eq!(request.0, &[3; 10]);

    let response_buf = [4; 20];
    assert!(sansio.handle(Response(&response_buf)).is_none());
}
