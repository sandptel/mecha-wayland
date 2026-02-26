use std::env;
use std::io::{self, BufWriter};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use tracing::{debug, info};

pub struct Connection {
    reader: UnixStream,
    writer: BufWriter<UnixStream>,
    next_id: u32,
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
        Ok(Connection { reader: stream, writer, next_id: 2 })
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

    pub fn recv_msg(&mut self) -> io::Result<(u32, u16, Vec<u8>)> {
        crate::wire::recv_msg(&mut self.reader)
    }

    pub fn flush(&mut self) -> io::Result<()> {
        use std::io::Write;
        self.writer.flush()
    }
}
