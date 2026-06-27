//! # asansio
//!
//! This library contains the async/await state machine for the sans-io design pattern. See [sans
//! I/O for network protocols](https://sans-io.readthedocs.io/) documentation to familiar with the
//! concept.  Writing network protocol without performing I/O operations means creating a state
//! machine.  Manually creating a state machine could be a tedious process. As Rust async/await
//! concept is an actual state machine with implicit states created during compilation, this
//! library is an experiment with using async/await to provide such state machine automatically.
//! Let's check if this is good solution to the problem.
//!
//! This crate could be used also for non network protocol cases, everywhere there is a need for
//! creating a state machine.
//!
//! This is `no_std` crate and it doesn't allocate on the heap.
//!
//! ## Usage
//!
//! You can provide async iface trait that can handle all SansIo handling. This way you can use
//! single protocol defined in async way with std or async runtime.
//!
//! This is the example of std interface and client:
//!
//! ```
//! use asansio::Io;
//! use asansio::IoHandle;
//! use asansio::Sans;
//! use std::pin::Pin;
//!
//! pub trait Iface {
//!     async fn recv(&mut self) -> Option<usize>;
//!     async fn send(&mut self, value: usize) -> Option<()>;
//! }
//!
//! async fn run(mut iface: impl Iface) -> Option<()> {
//!     loop {
//!         let value = iface.recv().await?;
//!         iface.send(value + 1).await?;
//!     }
//! }
//!
//! type Request = Option<usize>;
//! type Response = Option<usize>;
//!
//! struct IfaceStd {
//!     response: Option<usize>,
//!     sans: Sans<Request, Response>,
//! }
//!
//! impl Iface for IfaceStd {
//!     async fn recv(&mut self) -> Option<usize> {
//!        if let Some(response) = self.response.take() {
//!            return Some(response);
//!        }
//!        self.sans.handle(None).await.unwrap()
//!     }
//!
//!     async fn send(&mut self, value: usize) -> Option<()> {
//!        self.response = self.sans.handle(Some(value)).await.unwrap();
//!        self.response.map(|_| ())
//!     }
//! }
//!
//! struct ClientStd {
//!     io: Io<Request, Response>,
//!     handle: Option<IoHandle<Request, Response, Box<dyn Future<Output = ()>>>>,
//! }
//!
//! impl ClientStd {
//!     fn new() -> Option<Self> {
//!         let (sans, io) = asansio::new::<Request, Response>();
//!         let proto = Box::pin({
//!             async move {
//!                 run(IfaceStd { response: None, sans }).await.unwrap_or(());
//!             }
//!         }) as Pin<Box<dyn Future<Output = ()>>>;
//!         let (handle, request) = io.start(proto).ok().flatten()?;
//!         assert_eq!(request, None);
//!         let handle = Some(handle);
//!         Some(Self { io, handle })
//!     }
//!
//!    fn call(&mut self, value: usize) -> Option<usize> {
//!        let handle = self.handle.take().unwrap();
//!        let (handle, request) = self.io.handle(handle, Some(value)).unwrap()?;
//!        self.handle = Some(handle);
//!        request
//!    }
//! }
//!
//! let mut client = ClientStd::new().unwrap();
//!
//! assert_eq!(client.call(0), Some(1));
//! assert_eq!(client.call(2), Some(3));
//! assert_eq!(client.call(8), Some(9));
//! ```
//!
//! This crate divides a problem into two parts. The first `Sans` takes care of the state machine
//! independent of the I/O and the second `Io` is responsible with I/O communication.  There are
//! two types to manage them: the [Io] and the [Sans], which are constructed by the [new] function.
//! These two parts communicate using `Request` and `Respond` types, which are defined by the user
//! (for real scenarios they could be `enums`).
//!
//! `Sans` starts communicating with `Io` using [Sans::handle] and providing the initial `Request`;
//! it returns the `Response` from the `Io`.  `Io` starts sans task by using [Io::start] which
//! returns ([IoHandle], `Request`) from `Sans`. The later communication is done using
//! [Sans::handle] and [Io::handle].
//!
//! Possible design for the implementing protocol is to create an async interface trait, which
//! `Sans` will use as an interface to the `Io`. Application which uses async runtime can provide
//! implementation of this trait using runtime features and use `Sans` task directly . The one
//! which uses only `std` should implement the interface trait and use asansio crate for
//! communicating between that interface and main loop. See a similar design in
//! [examples](https://github.com/ewienik/asansio/tree/master/examples).
//!
//! ## Safety
//!
//! The crate uses `unsafe` parts for preparing a proper `Waker` with internal `Channel<Request,
//! Response>`. Safety is guaranteed by consuming the latest [IoHandle] and that you cannot mix
//! `Request` or `Response` types (it is guaranteed by generic types of [Io], [IoHandle] and
//! [Sans].  `Request` and `Response` types are consumed by values.

#![no_std]

use core::marker::PhantomData;
use core::mem;
use core::ops::Deref;
use core::ops::DerefMut;
use core::pin::Pin;
use core::ptr;
use core::task::Context;
use core::task::Poll;
use core::task::RawWaker;
use core::task::RawWakerVTable;
use core::task::Waker;

/// Store transmission message from(Tx) or to(Rx) Sans
#[derive(Default)]
enum Channel<Request, Response> {
    Tx(Request),
    Rx(Response),
    #[default]
    None,
}

impl<Request, Response> Channel<Request, Response> {
    fn tx(request: Request) -> Self {
        Self::Tx(request)
    }

    fn rx(response: Response) -> Self {
        Self::Rx(response)
    }

    fn take(&mut self) -> Self {
        mem::take(self)
    }
}

/// The Future helper for handling data between Io and Sans
struct SansFuture<Request, Response> {
    request: Option<Request>,
    _response: PhantomData<Response>,
    _not_send_sync: PhantomData<*const ()>,
}

impl<Request: Unpin, Response: Unpin> Future for SansFuture<Request, Response> {
    type Output = Result<Response, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker();
        if !ptr::eq(waker.vtable(), WAKER_VTABLE) {
            return Poll::Ready(Err(Error::Runtime));
        }

        // SAFETY: It is safe as Sans is !Send and Channel is valid for Io::run_async function
        let ch = unsafe { &mut *(waker.data() as *mut Channel<Request, Response>) };

        if let Some(request) = self.request.take() {
            *ch = Channel::tx(request);
            Poll::Pending
        } else {
            match ch.take() {
                // There is an answer from the Io part
                Channel::Rx(response) => Poll::Ready(Ok(response)),
                // There is still a request from the Sans part
                Channel::Tx(_) => Poll::Pending,
                // There is inconsistency, let's return error in Io's handle method
                Channel::None => Poll::Pending,
            }
        }
    }
}

/// Manages the Sans part
pub struct Sans<Request, Response> {
    _request: PhantomData<Request>,
    _response: PhantomData<Response>,
    _not_send_sync: PhantomData<*const ()>,
}

impl<Request: Unpin, Response: Unpin> Sans<Request, Response> {
    /// Next requests from the Sans part. It must receive SansHandle from the previous await call
    /// as the Response is not longer valid.
    pub fn handle(
        &self,
        request: Request,
    ) -> impl Future<Output = Result<Response, Error>> + use<Request, Response> {
        SansFuture {
            request: Some(request),
            _response: PhantomData,
            _not_send_sync: PhantomData,
        }
    }
}

/// Manages the Io part
pub struct Io<Request, Response> {
    _request: PhantomData<Request>,
    _response: PhantomData<Response>,
    _not_send_sync: PhantomData<*const ()>,
}

/// The holder of the Request from the Sans to Io
pub struct IoHandle<Request, Response, Task> {
    _request: PhantomData<Request>,
    _response: PhantomData<Response>,
    _not_send_sync: PhantomData<*const ()>,
    task: Pin<Task>,
}

impl<Request, Response> Io<Request, Response> {
    #[allow(clippy::type_complexity)]
    /// Starts the Sans part defined as a Future Task. Returns on the first async Request from Sans
    /// or when the Task finishes.
    pub fn start<Task>(
        &self,
        task: Pin<Task>,
    ) -> Result<Option<(IoHandle<Request, Response, Task>, Request)>, Error>
    where
        Task: DerefMut,
        <Task as Deref>::Target: Future<Output = ()>,
    {
        let mut handler = IoHandle {
            _request: PhantomData,
            _response: PhantomData,
            _not_send_sync: PhantomData,
            task,
        };
        let request = handler.run_async(Channel::<Request, Response>::None);
        request.map(|request| request.map(|request| (handler, request)))
    }

    #[allow(clippy::type_complexity)]
    /// Next polling of the Future Task of the Sans part. It must receive IoHandle from the
    /// previous await call as the Response is not longer valid. Returns on the Request from Sans
    /// or when the Task finishes.
    pub fn handle<Task>(
        &self,
        mut handler: IoHandle<Request, Response, Task>,
        response: Response,
    ) -> Result<Option<(IoHandle<Request, Response, Task>, Request)>, Error>
    where
        Task: DerefMut,
        <Task as Deref>::Target: Future<Output = ()>,
    {
        let request = handler.run_async(Channel::rx(response));
        request.map(|request| request.map(|request| (handler, request)))
    }
}

impl<Request, Response, Task> IoHandle<Request, Response, Task>
where
    Task: DerefMut,
    <Task as Deref>::Target: Future<Output = ()>,
{
    fn run_async(&mut self, mut ch: Channel<Request, Response>) -> Result<Option<Request>, Error> {
        // SAFETY: It is safe as ch: Channel is owned by this function, Sans is !Send, and
        // WAKER_VTABLE is noop
        let waker = unsafe { Waker::new(&mut ch as *mut _ as *mut (), WAKER_VTABLE) };

        let mut cx = Context::from_waker(&waker);
        match self.task.as_mut().poll(&mut cx) {
            Poll::Ready(_) => Ok(None),
            Poll::Pending => {
                if let Channel::Tx(request) = ch {
                    Ok(Some(request))
                } else {
                    Err(Error::Inconsistency)
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Inconsistency,
    Runtime,
}

/// Creates a two parts: Sans and Io for the specified Request and Response.
pub fn new<Request, Response>() -> (Sans<Request, Response>, Io<Request, Response>) {
    (
        Sans {
            _request: PhantomData,
            _response: PhantomData,
            _not_send_sync: PhantomData,
        },
        Io {
            _request: PhantomData,
            _response: PhantomData,
            _not_send_sync: PhantomData,
        },
    )
}

const WAKER_VTABLE: &RawWakerVTable = &RawWakerVTable::new(
    |data| RawWaker::new(data, WAKER_VTABLE),
    |_| {},
    |_| {},
    |_| {},
);

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadMe;
