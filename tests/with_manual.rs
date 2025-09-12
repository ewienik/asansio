mod proto;

use proto::Proto;
use sansio::SansIo;

#[test]
fn manual() {
    let proto = Proto::new();
    let mut sansio = SansIo::new();
    let request = sansio.start(proto.start(sansio.handler()));
}
