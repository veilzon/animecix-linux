use std::io;
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::thread::JoinHandle;

pub mod ygg;

use super::protocol::{read_frame, write_frame, Frame};
use super::types::PeerId;

/// Tek bir peer ile TCP bağlantısı. Yazma tarafı kilitli; okuma ayrı
/// thread'de çalışır ve çerçeveleri koordinatöre yollar.
pub struct PeerLink {
    writer: Mutex<TcpStream>,
}

impl PeerLink {
    pub fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        let writer = stream.try_clone()?;
        Ok(Self {
            writer: Mutex::new(writer),
        })
    }

    pub fn send(&self, frame: &Frame) -> io::Result<()> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "peer kilidi zehirlendi"))?;
        write_frame(&mut *w, frame)
    }

    pub fn reader(id: PeerId, stream: TcpStream, tx: Sender<(PeerId, Frame)>) -> JoinHandle<()> {
        std::thread::Builder::new()
            .name("watch-party-reader".into())
            .spawn(move || {
                let mut stream = stream;
                while let Ok(Some(frame)) = read_frame(&mut stream) {
                    if tx.send((id, frame)).is_err() {
                        return;
                    }
                }
                // EOF/okuma hatası: koordinatöre bağlantının koptuğunu bildir.
                let _ = tx.send((id, Frame::Json(super::protocol::WireMsg::Bye)));
            })
            .expect("watch-party reader thread")
    }
}

/// Yerel ağda dinlenecek adres. Yggdrasil geldiğinde bu katmanın yerini
/// `YggMesh` alacak; oturum kodu yalnızca `addr` string'ini bilir.
pub fn bind_listener() -> io::Result<TcpListener> {
    TcpListener::bind(("0.0.0.0", 0))
}

/// Dışarıya duyurulacak yerel IPv4 adresi. Ağ yoksa loopback.
pub fn local_ip() -> String {
    UdpSocket::bind(("0.0.0.0", 0))
        .and_then(|s| {
            s.connect(("8.8.8.8", 80))?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn advertised_addr(listener: &TcpListener) -> String {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    format!("{}:{port}", local_ip())
}

pub fn connect(addr: &str) -> io::Result<TcpStream> {
    let stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    Ok(stream)
}
