use core::pin::pin;
use sansio::Handler;
use sansio::SansIo;

#[test]
fn no_response() {
    struct Request;
    struct Response;

    let task = pin!(async {});
    assert!(SansIo::<Request, _>::start::<Response>(task).is_none());
}

#[test]
fn single_call() {
    struct Request;
    struct Response;

    let task = pin!(async {
        let handler = Handler::new();
        assert!(handler.response().is_none());

        let handler = handler.call(&Request).await;
        assert!(matches!(handler.response(), Some(&Response)));
    });

    let sansio = SansIo::start::<Response>(task).unwrap();
    assert!(matches!(sansio.request(), Some(&Request)));

    assert!(sansio.handle(&Response).is_none());
}

#[test]
fn send_owned_payload() {
    struct Request([u8; 10]);
    struct Response([u8; 20]);

    let task = pin!(async {
        let handler = Handler::new();

        let handler = handler.call(&Request([1; 10])).await;
        assert!(matches!(handler.response(), Some(&Response(_))));
        assert_eq!(handler.response().unwrap().0, [2; 20]);

        let handler = handler.call(&Request([3; 10])).await;
        assert!(matches!(handler.response(), Some(&Response(_))));
        assert_eq!(handler.response().unwrap().0, [4; 20]);
    });

    let sansio = SansIo::start::<Response>(task).unwrap();
    assert!(matches!(sansio.request(), Some(&Request(_))));
    assert_eq!(sansio.request().unwrap().0, [1; 10]);

    let sansio = sansio.handle(&Response([2; 20])).unwrap();
    assert!(matches!(sansio.request(), Some(&Request(_))));
    assert_eq!(sansio.request().unwrap().0, [3; 10]);

    assert!(sansio.handle(&Response([4; 20])).is_none());
}

#[test]
fn send_borrowed_payload() {
    struct Request<'a>(&'a [u8]);
    struct Response<'a>(&'a [u8]);

    let task = pin!(async {
        let handler = Handler::new();

        let mut request_buf = vec![0u8; 10];

        request_buf.fill(1);
        let handler = handler.call(&Request(&request_buf)).await;
        assert!(matches!(handler.response(), Some(&Response(_))));
        assert_eq!(handler.response().unwrap().0, [2; 20]);

        request_buf.fill(3);
        let handler = handler.call(&Request(&request_buf)).await;
        assert!(matches!(handler.response(), Some(&Response(_))));
        assert_eq!(handler.response().unwrap().0, [4; 20]);

        drop(request_buf);
        let mut request_buf = vec![0u8; 10];

        request_buf.fill(5);
        let handler = handler.call(&Request(&request_buf)).await;
        assert!(matches!(handler.response(), Some(&Response(_))));
        assert_eq!(handler.response().unwrap().0, [6; 20]);
    });

    let sansio = SansIo::start::<Response>(task).unwrap();
    assert!(matches!(sansio.request(), Some(&Request(_))));
    assert_eq!(sansio.request().unwrap().0, [1; 10]);

    let mut response_buf = vec![0; 20];

    response_buf.fill(2);
    let sansio = sansio.handle(&Response(&response_buf)).unwrap();
    assert!(matches!(sansio.request(), Some(&Request(_))));
    assert_eq!(sansio.request().unwrap().0, [3; 10]);

    response_buf.fill(4);
    let sansio = sansio.handle(&Response(&response_buf)).unwrap();
    assert!(matches!(sansio.request(), Some(&Request(_))));
    assert_eq!(sansio.request().unwrap().0, [5; 10]);

    drop(response_buf);
    let mut response_buf = vec![0; 20];

    response_buf.fill(6);
    assert!(sansio.handle(&Response(&response_buf)).is_none());
}
