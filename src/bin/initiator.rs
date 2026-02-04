use std::{env::var, os::unix::net::UnixStream, thread::spawn, time::Duration};

use mctp::{Eid, MsgType, ReqChannel};
use mctp_std::{Stack, util::update_loop};

use i2c_qemu_chardev_mctp_tools::{QemuI2cTransportReceiver, QemuI2cTransportSender};

// Compile time defaults

const UNIX_SOCKET_DEFAULT: &str = "vi2c_bus.sock"; // Set `UNIX_SOCKET` env variable to overwrite at runtime
const OWN_ADDR: u8 = 0x10;
const NEIGH_ADDR: u8 = 0x20;
// `PEC` is off by default, set env variable (`PEC`) to enable
const OWN_EID: Eid = Eid(9);
const REMOTE_EID: Eid = Eid(8);
const MSG_TYPE: MsgType = MsgType(1);
const TIMEOUT_SECS: u64 = 10;

fn main() {
    let pec = var("PEC").is_ok();
    if pec {
        println!("[INFO] PEC = true");
    }

    let sock_addr = var("UNIX_SOCKET").unwrap_or(UNIX_SOCKET_DEFAULT.to_owned());
    let socket = UnixStream::connect(sock_addr).expect("error opening socket");

    let sender = QemuI2cTransportSender {
        socket: socket.try_clone().unwrap(),
        own_addr: OWN_ADDR,
        dst_addr: NEIGH_ADDR,
        pec,
    };

    let mut stack = Stack::new(sender);
    stack.set_eid(OWN_EID).unwrap();

    let update_stack = stack.clone();
    spawn(move || update_loop(update_stack));

    let mut receiver = QemuI2cTransportReceiver {
        stack: stack.clone(),
        socket,
        own_addr: OWN_ADDR,
        pec,
    };

    spawn(move || receiver.run());

    // let mut listener = stack
    //     .listener(MSG_TYPE, Some(Duration::from_secs(TIMEOUT_SECS)))
    //     .unwrap();

    let mut request = stack
        .request(REMOTE_EID, Some(Duration::from_secs(TIMEOUT_SECS)))
        .unwrap();

    request
        .send(MSG_TYPE, "Hello World!".as_bytes())
        .expect("error sending request");

    let mut buf = [0; 256];
    let (_, _, msg) = request.recv(&mut buf).expect("error receiving response");

    println!("Got response: {:x?} ({:?})", msg, str::from_utf8(msg));
}
