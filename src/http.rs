use std::sync::{Arc, mpsc, Mutex, atomic::{AtomicU8, Ordering}};
use std::time::Duration;

enum Method {
    Get,
    Post,
    Head,
}

struct Spec {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    query: Option<Vec<(String, String)>>,
    json_body: Option<serde_json::Value>,
    timeout_secs: Option<u64>,
}

struct RawResp {
    status: u16,
    url: String,
    content_length: Option<u64>,
    body: Vec<u8>,
}

type Job = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send>;

struct Inner {
    primary: wreq::Client,
    fallback: Option<wreq::Client>,
    tx: mpsc::Sender<Job>,
    last_good: AtomicU8,
    /// Kullanıcının tarayıcıdan aldığı cf_clearance bileti. Boşsa takılmaz.
    /// Değer ASLA log'a yazılmaz.
    cf_clearance: Mutex<String>,
    /// Okul filtresi proxy'si (boşsa kapalı). Video hostlarına dokunulmaz,
    /// yalnız PROXY_HOSTS'taki API/kapak/kasa hostları yeniden yazılır.
    proxy_base: Mutex<String>,
}

/// Worker üzerinden taşınan hostlar (worker/proxy.js ALLOW ile birebir).
/// Bu listede olmayan host (sibnet/streamtape/CDN mp4/localhost) aynen geçer.
const PROXY_HOSTS: &[&str] = &[
    "animecix.tv",
    "tau-video.xyz",
    "image.tmdb.org",
    "raw.githubusercontent.com",
];

fn proxy_host_of(url: &str) -> &str {
    url.split("//").nth(1).unwrap_or("").split('/').next().unwrap_or("")
}

/// Proxy açıksa ve host listedeyse `{proxy}/{host}{path+query}` üretir.
/// Başlıklar HER ZAMAN özgün URL'ye göre hesaplanır (aşağıda rewrite'tan önce).
fn proxy_rewrite(proxy: &str, url: &str) -> Option<String> {
    let proxy = proxy.trim().trim_end_matches('/');
    if proxy.is_empty() {
        return None;
    }
    let host = proxy_host_of(url);
    if !PROXY_HOSTS.contains(&host.to_lowercase().as_str()) {
        return None;
    }
    let rest = url.split("//").nth(1).unwrap_or("");
    let path = rest.split_at(host.len()).1;
    Some(format!("{proxy}/{host}{path}"))
}

#[derive(Clone)]
pub(crate) struct Http {
    inner: Arc<Inner>,
}

pub(crate) struct Resp {
    raw: RawResp,
}

impl Resp {
    pub fn url(&self) -> String {
        self.raw.url.clone()
    }

    pub fn status(&self) -> u16 {
        self.raw.status
    }

    pub fn content_length(&self) -> Option<u64> {
        self.raw.content_length
    }

    pub fn error_for_status(self) -> Result<Self, String> {
        if (200..300).contains(&self.raw.status) {
            Ok(self)
        } else {
            Err(format!("HTTP {}", self.raw.status))
        }
    }

    pub fn text(self) -> Result<String, String> {
        String::from_utf8(self.raw.body).map_err(|_| "yanıt UTF-8 değil".to_string())
    }

    pub fn bytes(self) -> Result<Vec<u8>, String> {
        Ok(self.raw.body)
    }

    pub fn json<T: serde::de::DeserializeOwned>(self) -> Result<T, String> {
        serde_json::from_slice(&self.raw.body).map_err(|e| format!("JSON ayrıştırma hatası: {e}"))
    }
}

pub(crate) struct ReqB<'a> {
    http: &'a Http,
    spec: Spec,
}

impl ReqB<'_> {
    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.spec.headers.push((k.to_string(), v.to_string()));
        self
    }

    pub fn query(mut self, q: &[(&str, &str)]) -> Self {
        self.spec.query = Some(
            q.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        );
        self
    }

    pub fn json(mut self, v: &serde_json::Value) -> Self {
        self.spec.json_body = Some(v.clone());
        self
    }

    pub fn timeout(mut self, secs: u64) -> Self {
        self.spec.timeout_secs = Some(secs);
        self
    }

    pub fn send(self) -> Result<Resp, String> {
        let (tx, rx) = mpsc::channel::<Result<RawResp, String>>();
        let inner = self.http.inner.clone();
        let job_inner = inner.clone();
        let spec = self.spec;
        let job: Job = Box::new(move |rt| {
            let _ = tx.send(exec_cascade(rt, &job_inner, &spec));
        });
        inner
            .tx
            .send(job)
            .map_err(|_| "HTTP arka plan iş parçacığı kapandı".to_string())?;
        rx.recv()
            .map_err(|_| "HTTP arka plan iş parçacığı kapandı".to_string())?
            .map(|raw| Resp { raw })
    }
}

fn exec_on(rt: &tokio::runtime::Runtime, client: &wreq::Client, spec: &Spec, clearance: &str, proxy: &str) -> Result<RawResp, String> {
    let mut url = spec.url.clone();
    if let Some(q) = &spec.query {
        if !q.is_empty() {
            let qs: Vec<String> = q
                .iter()
                .map(|(k, v)| format!("{}={}", pct_encode(k), pct_encode(v)))
                .collect();
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&qs.join("&"));
        }
    }
    // Başlıklar özgün host'a göre (rewrite'tan ÖNCE).
    let mut extra: Vec<(String, String)> = browser_headers_for(&url, &spec.headers);
    if let Some(c) = cookie_header_for(&url, &spec.headers, clearance) {
        extra.push(("Cookie".to_string(), c));
    }
    // Proxy: yalnız izinli hostlar yeniden yazılır, diğerleri aynen geçer.
    if let Some(pu) = proxy_rewrite(proxy, &url) {
        url = pu;
    }
    let mut rb = match spec.method {
        Method::Get => client.get(&url),
        Method::Post => client.post(&url),
        Method::Head => client.head(&url),
    };
    for (k, v) in &spec.headers {
        rb = rb.header(k, v);
    }
    for (k, v) in &extra {
        rb = rb.header(k, v);
    }
    if let Some(j) = &spec.json_body {
        let body = serde_json::to_vec(j).map_err(|e| e.to_string())?;
        rb = rb.header("content-type", "application/json");
        rb = rb.body(body);
    }
    if let Some(t) = spec.timeout_secs {
        rb = rb.timeout(Duration::from_secs(t));
    }
    let resp = rt.block_on(rb.send()).map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    // Worker redirect'i kendi takip eder; nihai adresi başlıkta verir.
    let final_url = resp
        .headers()
        .get("x-final-url")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| resp.uri().to_string());
    let content_length = resp.content_length();
    let body = rt.block_on(resp.bytes()).map_err(|e| e.to_string())?.to_vec();
    Ok(RawResp {
        status,
        url: final_url,
        content_length,
        body,
    })
}

fn exec_cascade(rt: &tokio::runtime::Runtime, inner: &Inner, spec: &Spec) -> Result<RawResp, String> {
    let order: [(u8, &wreq::Client); 2] = if inner.last_good.load(Ordering::Relaxed) == 1 && inner.fallback.is_some() {
        [(1, inner.fallback.as_ref().unwrap()), (0, &inner.primary)]
    } else {
        [(0, &inner.primary), (1, inner.fallback.as_ref().unwrap_or(&inner.primary))]
    };

    let mut last: Option<Result<RawResp, String>> = None;
    for (idx, client) in order.iter().take(if inner.fallback.is_some() { 2 } else { 1 }) {
        let clearance = inner.cf_clearance.lock().map(|g| g.clone()).unwrap_or_default();
        let proxy = inner.proxy_base.lock().map(|g| g.clone()).unwrap_or_default();
        let res = exec_on(rt, client, &spec, &clearance, &proxy);
        match &res {
            Ok(r) if r.status != 403 => {
                inner.last_good.store(*idx, Ordering::Relaxed);
                return res;
            }
            _ => last = Some(res),
        }
    }
    last.unwrap_or_else(|| Err("yol yok".into()))
}

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'%' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

impl Http {
    pub fn new(tunnel_proxy: Option<&str>) -> Result<Self, String> {
        let mk = |proxy: Option<&str>| -> Result<wreq::Client, String> {
            let mut b = wreq::Client::builder()
                .emulation(wreq_util::Emulation::Chrome149)
                .timeout(Duration::from_secs(15))
                .connect_timeout(Duration::from_secs(5));
            if let Some(p) = proxy {
                let pr = wreq::Proxy::all(p).map_err(|e| e.to_string())?;
                b = b.proxy(pr);
            }
            b.build().map_err(|e| e.to_string())
        };
        let primary = mk(None)?;
        let fallback = match tunnel_proxy {
            Some(_) => Some(mk(tunnel_proxy)?),
            None => None,
        };
        let (tx, rx) = mpsc::channel::<Job>();
        let rt_primary = primary.clone();
        let rt_fallback = fallback.clone();
        std::thread::Builder::new()
            .name("http".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                for job in rx {
                    job(&rt);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            inner: Arc::new(Inner {
                primary: rt_primary,
                fallback: rt_fallback,
                tx,
                last_good: AtomicU8::new(0),
                cf_clearance: Mutex::new(String::new()),
                proxy_base: Mutex::new(String::new()),
            }),
        })
    }

    /// Okul filtresi proxy tabanı (boş string kapatır). Log'a yazılmaz.
    pub fn set_proxy(&self, base: &str) {
        let v = base.trim().trim_end_matches('/').to_string();
        let v = if v.len() > 512 { String::new() } else { v };
        if let Ok(mut g) = self.inner.proxy_base.lock() {
            *g = v;
        }
    }

    /// Cloudflare biletini kaydeder (boş string temizler). Log'a yazılmaz.
    pub fn set_cf_clearance(&self, v: &str) {
        let v = v.trim();
        if v.len() > 4096 {
            return;
        }
        if let Ok(mut g) = self.inner.cf_clearance.lock() {
            *g = v.to_string();
        }
    }

    pub fn get(&self, url: impl Into<String>) -> ReqB<'_> {
        ReqB {
            http: self,
            spec: Spec {
                method: Method::Get,
                url: url.into(),
                headers: Vec::new(),
                query: None,
                json_body: None,
                timeout_secs: None,
            },
        }
    }

    pub fn post(&self, url: impl Into<String>) -> ReqB<'_> {
        ReqB {
            http: self,
            spec: Spec {
                method: Method::Post,
                url: url.into(),
                headers: Vec::new(),
                query: None,
                json_body: None,
                timeout_secs: None,
            },
        }
    }

    pub fn head(&self, url: impl Into<String>) -> ReqB<'_> {
        ReqB {
            http: self,
            spec: Spec {
                method: Method::Head,
                url: url.into(),
                headers: Vec::new(),
                query: None,
                json_body: None,
                timeout_secs: None,
            },
        }
    }
}

/// animecix.tv'ye giden API isteklerine tarayıcı başlık seti üretir.
/// Video-host'larına (sibnet/tau/streamtape) dokunulmaz: onların kendi
/// referer/koruma mantığı var. Çağrı noktasında verilen başlıklar korunur.
///
/// NOT: UA + Sec-CH-UA* taklit katmanına aittir (tutarlı sürüm için);
/// buraya eklenmez. Origin de eklenmez (gerçek same-origin GET fetch'i
/// Origin göndermez). Sadece fetch-bağlamı + dil + referer verilir.
fn browser_headers_for(url: &str, existing: &[(String, String)]) -> Vec<(String, String)> {
    const DEFAULTS: [(&str, &str); 5] = [
        ("Referer", "https://animecix.tv/"),
        ("Accept-Language", "tr-TR,tr;q=0.9,en;q=0.8"),
        ("Sec-Fetch-Dest", "empty"),
        ("Sec-Fetch-Mode", "cors"),
        ("Sec-Fetch-Site", "same-origin"),
    ];
    let host = url
        .split("//")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");
    if host != "animecix.tv" {
        return Vec::new();
    }
    DEFAULTS
        .iter()
        .filter(|(k, _)| {
            !existing
                .iter()
                .any(|(ek, _)| ek.eq_ignore_ascii_case(k))
        })
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// animecix.tv isteklerine takılacak Cookie başlığının değerini üretir.
/// Boş bilet, video-host'ları ve çağrıda zaten Cookie varsa None döner.
fn cookie_header_for(
    url: &str,
    existing: &[(String, String)],
    clearance: &str,
) -> Option<String> {
    if clearance.is_empty() {
        return None;
    }
    let host = url
        .split("//")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");
    if host != "animecix.tv" {
        return None;
    }
    if existing
        .iter()
        .any(|(ek, _)| ek.eq_ignore_ascii_case("cookie"))
    {
        return None;
    }
    Some(format!("cf_clearance={clearance}"))
}

#[cfg(test)]
mod tests {
    use super::{browser_headers_for, cookie_header_for, proxy_rewrite};

    #[test]
    fn animecix_api_gets_browser_headers() {
        let h = browser_headers_for("https://animecix.tv/secure/search/frieren", &[]);
        let get = |k: &str| h.iter().find(|(ek, _)| ek == k).map(|(_, v)| v.clone());
        assert_eq!(get("Referer").as_deref(), Some("https://animecix.tv/"));
        assert_eq!(get("Sec-Fetch-Site").as_deref(), Some("same-origin"));
        assert_eq!(get("Sec-Fetch-Mode").as_deref(), Some("cors"));
        assert!(get("Sec-CH-UA").is_none(), "UA ipuçları taklite ait");
        assert!(get("Origin").is_none(), "GET fetch Origin taşımaz");
        assert_eq!(h.len(), 5);
    }

    #[test]
    fn explicit_headers_win() {
        let existing = vec![("Referer".to_string(), "https://ornek/".to_string())];
        let h = browser_headers_for("https://animecix.tv/secure/search/x", &existing);
        assert_eq!(h.len(), 4);
        assert!(h.iter().all(|(k, _)| k != "Referer"));
    }

    #[test]
    fn video_hosts_untouched() {
        for u in [
            "https://video.sibnet.ru/shell.php?videoid=1",
            "https://tau-video.xyz/api/video/abc",
            "https://streamtape.com/e/xyz",
        ] {
            assert!(browser_headers_for(u, &[]).is_empty(), "{u} bozulmamalı");
        }
    }

    #[test]
    fn proxy_rewrite_maps_allowlisted_hosts() {
        let p = "https://proxy.ornek";
        assert_eq!(
            proxy_rewrite(p, "https://animecix.tv/secure/search/naruto?limit=20").as_deref(),
            Some("https://proxy.ornek/animecix.tv/secure/search/naruto?limit=20")
        );
        assert_eq!(
            proxy_rewrite(p, "https://tau-video.xyz/api/video/abc?vid=1").as_deref(),
            Some("https://proxy.ornek/tau-video.xyz/api/video/abc?vid=1")
        );
        assert_eq!(
            proxy_rewrite(p, "https://image.tmdb.org/t/p/w185/x.jpg").as_deref(),
            Some("https://proxy.ornek/image.tmdb.org/t/p/w185/x.jpg")
        );
        assert_eq!(
            proxy_rewrite(p, "https://raw.githubusercontent.com/veilzon/a/main/f.json").as_deref(),
            Some("https://proxy.ornek/raw.githubusercontent.com/veilzon/a/main/f.json")
        );
        // Sondaki eğik çizgi yutulur, çift eğik çizgi çıkmaz.
        assert_eq!(
            proxy_rewrite("https://proxy.ornek/", "https://animecix.tv/secure/x").as_deref(),
            Some("https://proxy.ornek/animecix.tv/secure/x")
        );
    }

    #[test]
    fn proxy_rewrite_leaves_video_and_local_hosts() {
        let p = "https://proxy.ornek";
        for u in [
            "https://video.sibnet.ru/v/1/2.mp4",
            "https://sibnet.ru/shell.php?videoid=1",
            "https://streamtape.com/e/xyz",
            "https://cdn.ornek/v.mp4",
            "http://127.0.0.1:6800/jsonrpc",
            "https://evil.com/animecix.tv/secure/x",
        ] {
            assert!(proxy_rewrite(p, u).is_none(), "{u} değişmemeli");
        }
        assert!(proxy_rewrite("", "https://animecix.tv/secure/x").is_none());
        assert!(proxy_rewrite("   ", "https://animecix.tv/secure/x").is_none());
    }

    #[test]
    fn clearance_cookie_attached_to_api_only() {
        let v = cookie_header_for("https://animecix.tv/secure/search/x", &[], "BILET123");
        assert_eq!(v.as_deref(), Some("cf_clearance=BILET123"));
        assert!(cookie_header_for("https://animecix.tv/secure/search/x", &[], "").is_none());
        assert!(cookie_header_for("https://video.sibnet.ru/v/1/2.mp4", &[], "BILET123").is_none());
        assert!(cookie_header_for("https://tau-video.xyz/api/video/a", &[], "BILET123").is_none());
        let with_cookie = vec![("Cookie".to_string(), "a=b".to_string())];
        assert!(cookie_header_for("https://animecix.tv/secure/search/x", &with_cookie, "BILET123").is_none());
    }
}
