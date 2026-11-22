use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv6Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const DEFAULT_PEERS: &[&str] = &[
    "tcp://194.55.165.115:80?key=84a4e510bd5528d25cf17ac44d47168c4600110f32e8e8ca10d3c103f4e73aa4",
    "tcp://ygg1.mk16.de:1337?key=0000000087ee9949eeab56bd430ee8f324cad55abf3993ed9b9be63ce693e18a",
];

/// Uygulamayla birlikte gelen userspace `yggstack` yardımcısı.
/// Root/TUN yok; SOCKS5 üzerinden outbound, remote-tcp ile inbound.
pub struct YggHelper {
    child: Child,
    socks_port: u16,
    mesh_addr: String,
    ygg_port: u16,
    dir: PathBuf,
}

impl YggHelper {
    pub fn start_host(local_port: u16) -> Result<Self, String> {
        Self::start(local_port)
    }

    pub fn start_guest() -> Result<Self, String> {
        Self::start(0)
    }

    pub fn host_addr(&self) -> String {
        format!("[{}]:{}", self.mesh_addr, self.ygg_port)
    }

    pub fn connect(&self, addr: &str) -> io::Result<TcpStream> {
        let mut last = None;
        for _ in 0..60 {
            match socks5_connect(self.socks_port, addr) {
                Ok(s) => {
                    let _ = s.set_read_timeout(None);
                    return Ok(s);
                }
                Err(e) => {
                    last = Some(e);
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
        Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "ygg bağlantısı")))
    }

    fn start(local_port: u16) -> Result<Self, String> {
        let bin = helper_path()?;
        let socks_port = free_port()?;
        let ygg_port = if local_port > 0 {
            40_000 + (std::process::id() % 1000) as u16
        } else {
            0
        };
        let base = std::env::var("XDG_RUNTIME_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("animecix")
            .join("watch-party")
            .join(format!("{}-{}", std::process::id(), socks_port));
        fs::create_dir_all(&base).map_err(|e| format!("ygg dizini: {e}"))?;
        let conf_path = base.join("ygg.toml");

        let gen = Command::new(&bin)
            .arg("--genconf")
            .output()
            .map_err(|e| format!("yggstack başlatılamadı: {e}"))?;
        if !gen.status.success() {
            return Err(format!(
                "yggstack --genconf başarısız: {}",
                String::from_utf8_lossy(&gen.stderr).trim()
            ));
        }
        let mut text = String::from_utf8_lossy(&gen.stdout).into_owned();
        let peers = std::env::var("ANIMECIX_YGG_PEERS").unwrap_or_else(|_| {
            DEFAULT_PEERS
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>()
                .join(",")
        });
        let list = peers
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", ");
        text = text.replace("peers = []", &format!("peers = [{list}]"));
        fs::write(&conf_path, text).map_err(|e| format!("ygg config: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&conf_path, fs::Permissions::from_mode(0o600));
        }

        let addr_out = Command::new(&bin)
            .args(["--useconffile", &conf_path.to_string_lossy(), "--address"])
            .output()
            .map_err(|e| format!("ygg adresi: {e}"))?;
        let mesh_addr = String::from_utf8_lossy(&addr_out.stdout).trim().to_string();
        if mesh_addr.is_empty() || !mesh_addr.contains(':') {
            return Err("ygg adresi okunamadı".into());
        }

        let mut cmd = Command::new(&bin);
        cmd.args([
            "--useconffile",
            &conf_path.to_string_lossy(),
            "--socks",
            &format!("127.0.0.1:{socks_port}"),
        ]);
        if local_port > 0 {
            cmd.args([
                "--remote-tcp",
                &format!("{ygg_port}:127.0.0.1:{local_port}"),
            ]);
        }
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let child = cmd.spawn().map_err(|e| format!("yggstack: {e}"))?;
        std::thread::sleep(Duration::from_millis(350));
        Ok(Self {
            child,
            socks_port,
            mesh_addr,
            ygg_port,
            dir: base,
        })
    }
}

impl Drop for YggHelper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn helper_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("ANIMECIX_YGGSTACK") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["yggstack", "animecix-ygg"] {
                let p = dir.join(name);
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let p = dir.join("yggstack");
        if p.is_file() {
            return Ok(p);
        }
    }
    Err("yggstack yardımcısı bulunamadı".into())
}

fn free_port() -> Result<u16, String> {
    TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("port: {e}"))?
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| format!("port: {e}"))
}

fn socks5_connect(proxy_port: u16, target: &str) -> io::Result<TcpStream> {
    let (host, port) = parse_addr(target)?;
    let mut s = TcpStream::connect(("127.0.0.1", proxy_port))?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(Duration::from_secs(2)))?;
    s.write_all(&[0x05, 0x01, 0x00])?;
    let mut hello = [0u8; 2];
    s.read_exact(&mut hello)?;
    if hello != [0x05, 0x00] {
        return Err(io::Error::new(io::ErrorKind::Other, "SOCKS5 auth"));
    }
    let ip: Ipv6Addr = host
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "IPv6 adresi değil"))?;
    let mut req = vec![0x05, 0x01, 0x00, 0x04];
    req.extend_from_slice(&ip.octets());
    req.extend_from_slice(&port.to_be_bytes());
    s.write_all(&req)?;
    let mut head = [0u8; 4];
    s.read_exact(&mut head)?;
    if head[1] != 0x00 {
        return Err(io::Error::new(io::ErrorKind::Other, "SOCKS5 connect"));
    }
    match head[3] {
        0x01 => {
            let mut rest = [0u8; 6];
            s.read_exact(&mut rest)?;
        }
        0x04 => {
            let mut rest = [0u8; 18];
            s.read_exact(&mut rest)?;
        }
        0x03 => {
            let mut l = [0u8; 1];
            s.read_exact(&mut l)?;
            let mut rest = vec![0u8; l[0] as usize + 2];
            s.read_exact(&mut rest)?;
        }
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "SOCKS5 adres")),
    }
    Ok(s)
}

fn parse_addr(addr: &str) -> io::Result<(String, u16)> {
    let s = addr.trim();
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let (h, p) = rest
            .split_once(']')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "adres"))?;
        (h, p.trim_start_matches(':'))
    } else {
        let (h, p) = s
            .rsplit_once(':')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "adres"))?;
        (h, p)
    };
    let port = port
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "port"))?;
    Ok((host.to_string(), port))
}
