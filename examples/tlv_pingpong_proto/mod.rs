#![allow(dead_code)]

#[path = "../pingpong_proto/mod.rs"]
mod pingpong_proto;
#[path = "../tlv_proto/mod.rs"]
mod tlv_proto;

use asansio::Sans;
use pingpong_proto::ClientRequest as PpClientRequest;
use pingpong_proto::ClientResponse as PpClientResponse;
use pingpong_proto::ServerRequest as PpServerRequest;
use pingpong_proto::ServerResponse as PpServerResponse;
use std::pin::pin;
use std::time::Duration;
use tlv_proto::ClientRequest as TlvClientRequest;
use tlv_proto::ClientResponse as TlvClientResponse;
use tlv_proto::ServerRequest as TlvServerRequest;
use tlv_proto::ServerResponse as TlvServerResponse;

pub enum ClientRequest<'a> {
    ReadPayload,
    WritePayload { payload: &'a [u8] },
    Message { msg: &'a str },
    Error,
}

pub enum ClientResponse<'a> {
    ReadPayload { payload: &'a [u8] },
    Message { msg: &'a str },
    Sleep { duration: Duration },
}

pub enum ServerRequest<'a> {
    ReadPayload,
    WritePayload { payload: &'a [u8] },
    Sleep { duration: Duration },
    Error,
}

pub enum ServerResponse<'a> {
    ReadPayload { payload: &'a [u8] },
}

enum Client<'a> {
    Tlv(TlvClientResponse<'a>),
    Pp(PpClientResponse<'a>),
    Request(ClientRequest<'a>),
}

fn client_tlv_request<'a>(request: Option<&TlvClientRequest<'a>>) -> Option<Client<'a>> {
    request.map(|request| match request {
        TlvClientRequest::WritePayload { payload } => {
            Client::Request(ClientRequest::WritePayload { payload })
        }
        TlvClientRequest::Read { tag, val } => match tag {
            0 => Client::Pp(PpClientResponse::ReadMessage { payload: val }),
            _ => Client::Request(ClientRequest::Error),
        },
        TlvClientRequest::ReadPayload => Client::Request(ClientRequest::ReadPayload),
    })
}

fn client_pp_request<'a>(request: Option<&PpClientRequest<'a>>) -> Option<Client<'a>> {
    request.map(|request| match request {
        PpClientRequest::WriteMessage { payload } => Client::Tlv(TlvClientResponse::Write {
            tag: 0,
            val: payload,
        }),
        PpClientRequest::WriteSleep { payload } => Client::Tlv(TlvClientResponse::Write {
            tag: 1,
            val: payload,
        }),
        PpClientRequest::Message { msg } => Client::Request(ClientRequest::Message { msg }),
        PpClientRequest::Ready => Client::Tlv(TlvClientResponse::ReadPayload { payload: &[] }),
    })
}

fn client_response<'a>(response: Option<&ClientResponse<'a>>) -> Option<Client<'a>> {
    response.map(|response| match response {
        ClientResponse::ReadPayload { payload } => {
            Client::Tlv(TlvClientResponse::ReadPayload { payload })
        }
        ClientResponse::Message { msg } => Client::Pp(PpClientResponse::Message { msg }),
        ClientResponse::Sleep { duration } => Client::Pp(PpClientResponse::Sleep {
            duration: *duration,
        }),
    })
}

pub async fn run_client<'a>(sans: Sans<ClientRequest<'a>, ClientResponse<'a>>) {
    let (pp_sans, pp_io) = asansio::new();
    let pp_task = pin!(pingpong_proto::run_client(pp_sans));

    let (tlv_sans, tlv_io) = asansio::new();
    let tlv_task = pin!(tlv_proto::run_client(tlv_sans));

    let mut pp_io_request = pp_io.start(pp_task);
    if pp_io_request.is_none() {
        return;
    }
    let mut tlv_io_request = tlv_io.start(tlv_task);
    if tlv_io_request.is_none() {
        return;
    };

    let mut sans_resp = sans.start(&ClientRequest::ReadPayload).await;
    let mut client = client_response(sans_resp.response());
    loop {
        client = match client {
            Some(Client::Tlv(response)) => {
                tlv_io_request = tlv_io.handle(tlv_io_request.take().unwrap(), &response);
                if tlv_io_request.is_none() {
                    return;
                };
                client_tlv_request(tlv_io_request.as_ref().unwrap().request())
            }

            Some(Client::Pp(response)) => {
                pp_io_request = pp_io.handle(pp_io_request.take().unwrap(), &response);
                if pp_io_request.is_none() {
                    return;
                };
                client_pp_request(pp_io_request.as_ref().unwrap().request())
            }

            Some(Client::Request(request)) => {
                sans_resp = sans.handle(sans_resp, &request).await;
                client_response(sans_resp.response())
            }

            None => break,
        };
    }
}

enum Server<'a> {
    Tlv(TlvServerResponse<'a>),
    Pp(PpServerResponse<'a>),
    Request(ServerRequest<'a>),
}

fn server_tlv_request<'a>(request: Option<&TlvServerRequest<'a>>) -> Option<Server<'a>> {
    request.map(|request| match request {
        TlvServerRequest::WritePayload { payload } => {
            Server::Request(ServerRequest::WritePayload { payload })
        }
        TlvServerRequest::Read { tag, val } => match tag {
            0 => Server::Pp(PpServerResponse::ReadMessage { payload: val }),
            1 => Server::Pp(PpServerResponse::ReadSleep { payload: val }),
            _ => Server::Request(ServerRequest::Error),
        },
        TlvServerRequest::ReadPayload => Server::Request(ServerRequest::ReadPayload),
    })
}

fn server_pp_request<'a>(request: Option<&PpServerRequest<'a>>) -> Option<Server<'a>> {
    request.map(|request| match request {
        PpServerRequest::WriteMessage { payload } => Server::Tlv(TlvServerResponse::Write {
            tag: 0,
            val: payload,
        }),
        PpServerRequest::Sleep { duration } => Server::Request(ServerRequest::Sleep {
            duration: *duration,
        }),
        PpServerRequest::Read => Server::Tlv(TlvServerResponse::ReadPayload { payload: &[] }),
    })
}

fn server_response<'a>(response: Option<&ServerResponse<'a>>) -> Option<Server<'a>> {
    response.map(|response| match response {
        ServerResponse::ReadPayload { payload } => {
            Server::Tlv(TlvServerResponse::ReadPayload { payload })
        }
    })
}

pub async fn run_server<'a>(sans: Sans<ServerRequest<'a>, ServerResponse<'a>>) {
    let (pp_sans, pp_io) = asansio::new();
    let pp_task = pin!(pingpong_proto::run_server(pp_sans));

    let (tlv_sans, tlv_io) = asansio::new();
    let tlv_task = pin!(tlv_proto::run_server(tlv_sans));

    let mut pp_io_request = pp_io.start(pp_task);
    if pp_io_request.is_none() {
        return;
    }
    let mut tlv_io_request = tlv_io.start(tlv_task);
    if tlv_io_request.is_none() {
        return;
    };

    let mut sans_resp = sans.start(&ServerRequest::ReadPayload).await;
    let mut server = server_response(sans_resp.response());
    loop {
        server = match server {
            Some(Server::Tlv(response)) => {
                tlv_io_request = tlv_io.handle(tlv_io_request.take().unwrap(), &response);
                if tlv_io_request.is_none() {
                    return;
                };
                server_tlv_request(tlv_io_request.as_ref().unwrap().request())
            }

            Some(Server::Pp(response)) => {
                pp_io_request = pp_io.handle(pp_io_request.take().unwrap(), &response);
                if pp_io_request.is_none() {
                    return;
                };
                server_pp_request(pp_io_request.as_ref().unwrap().request())
            }

            Some(Server::Request(request)) => {
                sans_resp = sans.handle(sans_resp, &request).await;
                server_response(sans_resp.response())
            }

            None => break,
        };
    }
}
