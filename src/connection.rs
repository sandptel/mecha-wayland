use std::collections::VecDeque;
use std::env;
use std::io::{self, BufWriter, Read};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use tracing::{debug, info};

pub struct Connection {
    reader: UnixStream,
    writer: BufWriter<UnixStream>,
    next_id: u32,
    pending_fds: VecDeque<OwnedFd>,
}

impl Connection {
    pub fn connect() -> io::Result<Self> {
        let path = Self::resolve_socket_path()?;
        info!(path = ?path, "connecting to Wayland socket");
        Self::connect_to(&path)
    }

    pub fn connect_to(path: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        let writer = BufWriter::new(stream.try_clone()?);
        Ok(Connection { reader: stream, writer, next_id: 2, pending_fds: VecDeque::new() })
    }

    fn resolve_socket_path() -> io::Result<PathBuf> {
        let runtime_dir = env::var("XDG_RUNTIME_DIR")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR not set"))?;
        let display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());

        if display.starts_with('/') {
            Ok(PathBuf::from(display))
        } else {
            let mut path = PathBuf::from(runtime_dir);
            path.push(display);
            Ok(path)
        }
    }

    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        debug!(id, "allocated object id");
        id
    }

    pub fn send_msg(&mut self, object_id: u32, opcode: u16, args: &[u8]) -> io::Result<()> {
        crate::wire::send_msg(&mut self.writer, object_id, opcode, args)
    }

    pub fn send_msg_with_fds(
        &mut self,
        object_id: u32,
        opcode: u16,
        args: &[u8],
        fds: &[RawFd],
    ) -> io::Result<()> {
        use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
        use std::io::{IoSlice, Write};

        self.writer.flush()?;

        let total_size = 8 + args.len();
        let word2 = ((total_size as u32) << 16) | (opcode as u32);
        let mut buf = Vec::with_capacity(total_size);
        buf.extend_from_slice(&object_id.to_le_bytes());
        buf.extend_from_slice(&word2.to_le_bytes());
        buf.extend_from_slice(args);

        let iov = [IoSlice::new(&buf)];
        let cmsg = [ControlMessage::ScmRights(fds)];
        tracing::trace!(object_id, opcode, bytes = total_size, fds = fds.len(), "→ sendmsg");
        sendmsg::<nix::sys::socket::UnixAddr>(
            self.reader.as_raw_fd(),
            &iov,
            &cmsg,
            MsgFlags::empty(),
            None,
        )
        .map_err(|e| io::Error::from_raw_os_error(e as i32))?;
        Ok(())
    }

    pub fn recv_msg(&mut self) -> io::Result<(u32, u16, Vec<u8>)> {
        use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};
        use std::io::IoSliceMut;

        let mut header = [0u8; 8];
        let mut iov = [IoSliceMut::new(&mut header)];
        let mut cmsg_buf = nix::cmsg_space!([RawFd; 28]);

        let msg = recvmsg::<nix::sys::socket::UnixAddr>(
            self.reader.as_raw_fd(),
            &mut iov,
            Some(&mut cmsg_buf),
            MsgFlags::empty(),
        )
        .map_err(|e| io::Error::from_raw_os_error(e as i32))?;

        for cmsg in msg.cmsgs().map_err(|e| io::Error::from_raw_os_error(e as i32))? {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                for fd in fds {
                    self.pending_fds.push_back(unsafe { OwnedFd::from_raw_fd(fd) });
                }
            }
        }

        let object_id = u32::from_le_bytes(header[0..4].try_into().unwrap());
        let word2 = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let total_len = (word2 >> 16) as usize;
        let opcode = (word2 & 0xffff) as u16;
        let body_len = total_len - 8;
        let mut body = vec![0u8; body_len];
        self.reader.read_exact(&mut body)?;

        tracing::trace!(object_id, opcode, bytes = total_len, "← recvmsg");
        Ok((object_id, opcode, body))
    }

    pub fn pop_fd(&mut self) -> io::Result<OwnedFd> {
        self.pending_fds
            .pop_front()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "no fd in queue"))
    }

    pub fn flush(&mut self) -> io::Result<()> {
        use std::io::Write;
        self.writer.flush()
    }
}
