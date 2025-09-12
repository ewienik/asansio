mod proto;

use proto::Proto;
use proto::Read;
use proto::Response;
use sansio::SansIo;

#[test]
fn manual() {
    let proto = Proto::new();
    let mut sansio = SansIo::new();
    let request = sansio.start(proto.start(sansio.handler()));
    let request = sansio.handle(Response::Read(Read));
}
