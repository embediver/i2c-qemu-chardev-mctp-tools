use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;

use mctp::{Eid, Result, Tag};
use mctp_lib::fragment::SendOutput;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use mctp_lib::Sender;
use mctp_lib::i2c::MctpI2cEncap;

pub struct QemuI2cTransportSender {
    pub socket: UnixStream,
    pub own_addr: u8,
    pub dst_addr: u8,
    pub pec: bool,
}

impl QemuI2cTransportSender {
    fn send_fragment(&mut self, fragment: &[u8]) -> Result<()> {
        let codec = MctpI2cEncap::new(self.own_addr);
        let mut codec_buf = [0; mctp_lib::i2c::MCTP_I2C_MAXMTU];
        let i2c_pkt = codec.encode(self.dst_addr, fragment, &mut codec_buf, self.pec)?;

        let qemu_header =
            QemuI2cChardevHeader::new(i2c_pkt.len() as u16, self.own_addr, self.dst_addr);

        let mut send_buf = vec![0; size_of::<QemuI2cChardevHeader>() + i2c_pkt.len()];
        let (head, data) = send_buf
            .as_mut_slice()
            .split_at_mut(size_of::<QemuI2cChardevHeader>());

        qemu_header
            .write_to(head)
            .map_err(|_| mctp::Error::InternalError)?;
        data.copy_from_slice(i2c_pkt);

        self.socket
            .write_all(&send_buf)
            .map_err(|_| mctp::Error::TxFailure)?;
        Ok(())
    }
}

impl Sender for QemuI2cTransportSender {
    fn send_vectored(
        &mut self,
        _eid: Eid,
        mut fragmenter: mctp_lib::fragment::Fragmenter,
        payload: &[&[u8]],
    ) -> Result<Tag> {
        loop {
            let mut pkt = [0; mctp_lib::i2c::MCTP_I2C_MAXMTU];
            let fragment = fragmenter.fragment_vectored(payload, &mut pkt);
            match fragment {
                SendOutput::Packet(items) => {
                    self.send_fragment(items)?;
                }
                SendOutput::Complete { tag, cookie: _ } => return Ok(tag),
                SendOutput::Error { err, cookie: _ } => return Err(err),
            }
        }
    }

    fn get_mtu(&self) -> usize {
        mctp_lib::i2c::MCTP_I2C_MAXMTU
    }
}

/// As defined in qemu/include/hw/i2c/chardev_i2c.h
#[repr(C)]
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
struct QemuI2cChardevHeader {
    magic: u8,
    version: u8,
    len: u16,
    src_addr: u8,
    dst_addr: u8,
}

impl QemuI2cChardevHeader {
    const MAGIC: u8 = 0xCD;
    const VERSION: u8 = 0x01;
    fn new(len: u16, src_addr: u8, dst_addr: u8) -> QemuI2cChardevHeader {
        QemuI2cChardevHeader {
            magic: Self::MAGIC,
            version: Self::VERSION,
            len,
            src_addr,
            dst_addr,
        }
    }
}

pub struct QemuI2cTransportReceiver {
    pub stack: mctp_std::Stack<QemuI2cTransportSender>,
    pub socket: UnixStream,
    pub own_addr: u8,
    pub pec: bool,
}

impl QemuI2cTransportReceiver {
    pub fn run(&mut self) {
        loop {
            let mut header_buf = [0; size_of::<QemuI2cChardevHeader>()];
            match self.socket.read_exact(&mut header_buf) {
                Ok(_) => {}
                Err(e) => {
                    if e.kind() == ErrorKind::UnexpectedEof {
                        return;
                    } else {
                        panic!("failed to read from stream: {e:?}");
                    }
                }
            }

            let header = QemuI2cChardevHeader::ref_from_bytes(&header_buf)
                .expect("Failed to parse chardev transport header!?");
            assert_eq!(
                header.magic,
                QemuI2cChardevHeader::MAGIC,
                "Invalid MAGIC for qemu chardev header"
            );
            assert_eq!(
                header.version,
                QemuI2cChardevHeader::VERSION,
                "Invalid VERSION for qemu chardev header ({:#02x})",
                header.version
            );

            let mut i2c_pkt = vec![0; header.len as usize];
            self.socket.read_exact(&mut i2c_pkt).unwrap();

            if header.dst_addr != self.own_addr {
                // Discard packet if header address does not match
                println!(
                    "[WARN] Discarding packet with wrong destination addr ({:#02x})",
                    header.dst_addr
                );
                continue;
            }

            // Successfully received a packet over the qemu i2c chardev socket.
            // Further decoding errors will just be printed instead of crashing the application.

            let codec = MctpI2cEncap::new(self.own_addr);

            let (pkt, header) = match codec.decode(&i2c_pkt, self.pec) {
                Ok(ret) => ret,
                Err(e) => {
                    println!("[ERROR] decoding I2C packet: {e:?}");
                    continue;
                }
            };
            if header.dest != self.own_addr {
                // Discard packet if header address does not match
                println!(
                    "[ERROR] chardev header destination does not match I2C transport destination ({:#02x}), discarding",
                    header.dest
                );
                continue;
            }

            self.stack
                .inbound(pkt)
                .inspect_err(|e| println!("[ERROR] processing inbound packet: {e}"))
                .ok();
        }
    }
}
