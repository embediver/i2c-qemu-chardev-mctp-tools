use std::{
    env::var,
    os::unix::net::{UnixListener, UnixStream},
    sync::OnceLock,
    thread::spawn,
    time::Duration,
};

use mctp::{Eid, Listener, MsgType, RespChannel};
use mctp_std::{Stack, util::update_loop};

use i2c_qemu_chardev_mctp_tools::{QemuI2cTransportReceiver, QemuI2cTransportSender};

// Compile time defaults

const UNIX_SOCKET_DEFAULT: &str = "vi2c_bus.sock"; // Set `UNIX_SOCKET` env variable to overwrite at runtime
/// Set `SERVER` env variable to `true` or `1` to enable, `false` to disable
const SERVER: bool = true;
const OWN_ADDR: u8 = 0x20;
const NEIGH_ADDR: u8 = 0x10;
// `PEC` is off by default, set env variable (`PEC`) to enable
const OWN_EID: Eid = Eid(8);
const MSG_TYPE: MsgType = MsgType(1);
const TIMEOUT_SECS: u64 = 10;

static IS_SERVER: OnceLock<bool> = OnceLock::new();

fn main() {
    let pec = var("PEC").is_ok();

    let socket = open_socket();

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
        socket: socket.try_clone().unwrap(),
        own_addr: OWN_ADDR,
        pec,
    };

    spawn(move || receiver.run());

    let mut listener = stack
        .listener(MSG_TYPE, Some(Duration::from_secs(TIMEOUT_SECS)))
        .unwrap();

    let mut buf = [0; 256];
    let (_, _, msg, mut rsp) = listener.recv(&mut buf).unwrap();

    println!("Got message: {:x?} ({:?})", msg, str::from_utf8(msg));

    rsp.send(msg).unwrap();

    socket
        .shutdown(std::net::Shutdown::Both)
        .expect("Failed to shutdown socket");
    #[allow(clippy::collapsible_if)]
    if IS_SERVER.get().is_some_and(|inner| *inner) {
        if let Some(path) = socket.local_addr().unwrap().as_pathname() {
            std::fs::remove_file(path).expect("failed to remove socket");
        }
    }
}

/// Opens a socket, either as server or client
fn open_socket() -> UnixStream {
    let sock_addr = var("UNIX_SOCKET").unwrap_or(UNIX_SOCKET_DEFAULT.to_owned());

    let server = if let Ok(server_env) = var("SERVER") {
        matches!(server_env.as_str(), "true" | "1")
    } else {
        SERVER
    };
    IS_SERVER.get_or_init(|| server);

    if server {
        let server = UnixListener::bind(sock_addr).expect("error opening socket (server)");
        server.accept().unwrap().0
    } else {
        UnixStream::connect(sock_addr).expect("error opening socket")
    }
}
