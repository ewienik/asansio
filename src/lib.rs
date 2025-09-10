use futures::future::LocalBoxFuture;
use futures::FutureExt;

pub struct Id(usize);

pub struct SansIo<T> {
    tasks: Vec<usize>,
    inner: T,
}

pub struct SansIoHandler<'a, Request, Response> {
    tasks: Vec<LocalBoxFuture<'a, ()>>,
    request: Option<Request>,
}

impl<'a, Request, Response> SansIoHandler<'a, Request, Response> {
    pub fn spawn(&mut self, f: impl Future<Output = ()> + 'a) -> Result<(), SansIoError> {
        self.tasks.push(f.boxed_local());
        Ok(())
    }

    pub fn call(&mut self, request: Request) -> SansIoCall<Response> {
        SansIoCall {}
    }
}

pub struct SansIoCall<Response> {}

impl<Response> Future for SansIoCall<Response> {
    type Output = Response;

    fn poll(self: Pin(&mut Self), _: &mut Context<'_>) -> Poll<Self::Output> {
        todo!()
    }
}

pub trait SansIoMachine {
    type Request;
    type Response;
    type Error;

    async fn start(&self, sansio: &mut SansIoHandler<Self::Request>);
    async fn handle(
        &self,
        sansio: &mut SansIoHandler<Self::Request>,
        id: Id,
        response: Self::Response,
    );
}

impl<T: SansIoMachine> SansIo<T> {
    pub fn new(inner: T) -> Self {
        Self {
            tasks: Vec::new(),
            inner,
        }
    }

    pub fn start(&mut self) -> Vec<(Id, T::Request)> {
        Vec::new()
    }

    pub fn handle(
        &mut self,
        id: Id,
        _response: T::Response,
    ) -> Result<Vec<T::Request>, SansIoError> {
        let requests = Vec::new();
        if id.0 >= self.tasks.len() {
            return Err(SansIoError::TaskNotExists);
        }
        Ok(requests)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SansIoError {
    #[error("task not exists")]
    TaskNotExists,
}
