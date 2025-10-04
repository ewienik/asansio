#[path = "../pingpong_proto/mod.rs"]
mod pingpong_proto;
#[path = "../tlv_proto/mod.rs"]
mod tlv_proto;

use bytes::Bytes;
use pingpong_proto::ClientRequest as PpClientRequest;
use pingpong_proto::ClientResponse as PpClientResponse;
use pingpong_proto::ServerRequest as PpServerRequest;
use pingpong_proto::ServerResponse as PpServerResponse;
use sansio::Sans;
use std::pin::pin;
use std::time::Duration;
use tlv_proto::ClientRequest as TlvClientRequest;
use tlv_proto::ClientResponse as TlvClientResponse;
use tlv_proto::ServerRequest as TlvServerRequest;
use tlv_proto::ServerResponse as TlvServerResponse;

pub enum ClientRequest {
    ReadPayload,
    WritePayload { payload: Bytes },
    Message { msg: String },
    Error,
}

pub enum ClientResponse {
    ReadPayload { payload: Bytes },
    Message { msg: String },
    Sleep { duration: Duration },
}

pub enum ServerRequest {
    ReadPayload,
    WritePayload { payload: Bytes },
    Sleep { duration: Duration },
    Error,
}

pub enum ServerResponse {
    ReadPayload { payload: Bytes },
}

enum Client {
    Tlv(TlvClientResponse),
    Pp(PpClientResponse),
    Request(ClientRequest),
}

fn client_tlv_request(request: Option<&TlvClientRequest>) -> Option<Client> {
    request.map(|request| match request {
        TlvClientRequest::WritePayload { payload } => {
            Client::Request(ClientRequest::WritePayload {
                payload: payload.clone(),
            })
        }
        TlvClientRequest::Read { tag, val } => match tag {
            0 => Client::Pp(PpClientResponse::ReadMessage {
                payload: val.clone(),
            }),
            _ => Client::Request(ClientRequest::Error),
        },
        TlvClientRequest::ReadPayload => Client::Request(ClientRequest::ReadPayload),
    })
}

fn client_pp_request(request: Option<&PpClientRequest>) -> Option<Client> {
    request.map(|request| match request {
        PpClientRequest::WriteMessage { payload } => Client::Tlv(TlvClientResponse::Write {
            tag: 0,
            val: payload.clone(),
        }),
        PpClientRequest::WriteSleep { payload } => Client::Tlv(TlvClientResponse::Write {
            tag: 1,
            val: payload.clone(),
        }),
        PpClientRequest::Message { msg } => {
            Client::Request(ClientRequest::Message { msg: msg.clone() })
        }
        PpClientRequest::Ready => Client::Tlv(TlvClientResponse::ReadPayload {
            payload: Bytes::new(),
        }),
    })
}

fn client_response(response: Option<&ClientResponse>) -> Option<Client> {
    response.map(|response| match response {
        ClientResponse::ReadPayload { payload } => Client::Tlv(TlvClientResponse::ReadPayload {
            payload: payload.clone(),
        }),
        ClientResponse::Message { msg } => {
            Client::Pp(PpClientResponse::Message { msg: msg.clone() })
        }
        ClientResponse::Sleep { duration } => Client::Pp(PpClientResponse::Sleep {
            duration: *duration,
        }),
    })
}

pub async fn run_client(sans: Sans<ClientRequest, ClientResponse>) {
    let (pp_sans, pp_io) = sansio::new();
    let pp_task = pin!(pingpong_proto::run_client(pp_sans));

    let (tlv_sans, tlv_io) = sansio::new();
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

enum Server {
    Tlv(TlvServerResponse),
    Pp(PpServerResponse),
    Request(ServerRequest),
}

fn server_tlv_request(request: Option<&TlvServerRequest>) -> Option<Server> {
    request.map(|request| match request {
        TlvServerRequest::WritePayload { payload } => {
            Server::Request(ServerRequest::WritePayload {
                payload: payload.clone(),
            })
        }
        TlvServerRequest::Read { tag, val } => match tag {
            0 => Server::Pp(PpServerResponse::ReadMessage {
                payload: val.clone(),
            }),
            1 => Server::Pp(PpServerResponse::ReadSleep {
                payload: val.clone(),
            }),
            _ => Server::Request(ServerRequest::Error),
        },
        TlvServerRequest::ReadPayload => Server::Request(ServerRequest::ReadPayload),
    })
}

fn server_pp_request(request: Option<&PpServerRequest>) -> Option<Server> {
    request.map(|request| match request {
        PpServerRequest::WriteMessage { payload } => Server::Tlv(TlvServerResponse::Write {
            tag: 0,
            val: payload.clone(),
        }),
        PpServerRequest::Sleep { duration } => Server::Request(ServerRequest::Sleep {
            duration: *duration,
        }),
        PpServerRequest::Read => Server::Tlv(TlvServerResponse::ReadPayload {
            payload: Bytes::new(),
        }),
    })
}

fn server_response(response: Option<&ServerResponse>) -> Option<Server> {
    response.map(|response| match response {
        ServerResponse::ReadPayload { payload } => Server::Tlv(TlvServerResponse::ReadPayload {
            payload: payload.clone(),
        }),
    })
}

pub async fn run_server(sans: Sans<ServerRequest, ServerResponse>) {
    let (pp_sans, pp_io) = sansio::new();
    let pp_task = pin!(pingpong_proto::run_server(pp_sans));

    let (tlv_sans, tlv_io) = sansio::new();
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
