mod proto;

use proto::Proto;
use sansio::SansIo;

#[test]
fn manual() {
    let mut sansio = SansIo::new(Proto::new());
    let requests = sansio.start();
}
