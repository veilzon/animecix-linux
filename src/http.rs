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
    headers: Vec<(String, String)>,
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
    connect_sid: Mutex<String>,
    xsrf_token: Mutex<String>,
    api_token: Mutex<String>,
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

    pub fn headers(&self) -> &[(String, String)] {
        &self.raw.headers
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.raw
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn set_cookie_val(&self, cookie_name: &str) -> Option<String> {
        let prefix = format!("{cookie_name}=");
        for (k, v) in &self.raw.headers {
            if k.eq_ignore_ascii_case("set-cookie") {
                if let Some(pos) = v.find(&prefix) {
                    let rest = &v[pos + prefix.len()..];
                    let val = rest.split(';').next().unwrap_or("").trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
        None
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

fn exec_on(
    rt: &tokio::runtime::Runtime,
    client: &wreq::Client,
    spec: &Spec,
    clearance: &str,
    connect_sid: &str,
    xsrf_token: &str,
    api_token: &str,
) -> Result<RawResp, String> {
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
    // Başlıklar özgün host'a ve mobil API'ye göre.
    let is_mobile_api = url.contains("/api/v1/");
    let mut extra: Vec<(String, String)> = if is_mobile_api {
        let mut h = Vec::new();
        h.push(("User-Agent".to_string(), "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36 ANIMECIXMOBILE".to_string()));
        h.push(("Accept".to_string(), "application/json".to_string()));
        if !api_token.is_empty() {
            h.push(("Authorization".to_string(), format!("Bearer {api_token}")));
        }
        h
    } else {
        browser_headers_for(&url, &spec.headers)
    };

    if !is_mobile_api {
        if let Some(c) = cookie_header_for(&url, &spec.headers, clearance, connect_sid, xsrf_token) {
            extra.push(("Cookie".to_string(), c));
        }
        let is_animecix = url.contains("animecix.tv");
        if is_animecix && !xsrf_token.is_empty() && !spec.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-xsrf-token")) {
            extra.push(("X-XSRF-TOKEN".to_string(), xsrf_token.to_string()));
        }
        if is_animecix && matches!(spec.method, Method::Post) && !spec.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("origin")) {
            extra.push(("Origin".to_string(), "https://animecix.tv".to_string()));
        }
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
    let final_url = resp.uri().to_string();
    let content_length = resp.content_length();
    let mut resp_headers = Vec::new();
    for (name, value) in resp.headers() {
        if let Ok(v) = value.to_str() {
            resp_headers.push((name.as_str().to_string(), v.to_string()));
        }
    }
    let body = rt.block_on(resp.bytes()).map_err(|e| e.to_string())?.to_vec();
    Ok(RawResp {
        status,
        url: final_url,
        content_length,
        headers: resp_headers,
        body,
    })
}

fn extract_cookie_val(headers: &[(String, String)], cookie_name: &str) -> Option<String> {
    let prefix = format!("{cookie_name}=");
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("set-cookie") {
            if let Some(pos) = v.find(&prefix) {
                let rest = &v[pos + prefix.len()..];
                let val = rest.split(';').next().unwrap_or("").trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
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
        let connect_sid = inner.connect_sid.lock().map(|g| g.clone()).unwrap_or_default();
        let mut xsrf_token = inner.xsrf_token.lock().map(|g| g.clone()).unwrap_or_default();
        let api_token = inner.api_token.lock().map(|g| g.clone()).unwrap_or_default();

        let is_mobile = spec.url.contains("/api/v1/");

        if !is_mobile && xsrf_token.len() < 10 && spec.url.contains("animecix.tv") && matches!(spec.method, Method::Post) {
            let bs_spec = Spec {
                method: Method::Get,
                url: "https://animecix.tv/secure/bootstrap-data".to_string(),
                headers: Vec::new(),
                query: None,
                json_body: None,
                timeout_secs: Some(5),
            };
            if let Ok(bs_resp) = exec_on(rt, client, &bs_spec, &clearance, &connect_sid, "", "") {
                if let Some(tok) = extract_cookie_val(&bs_resp.headers, "XSRF-TOKEN") {
                    if let Ok(mut g) = inner.xsrf_token.lock() {
                        *g = tok.clone();
                    }
                    xsrf_token = tok;
                }
            }
        }

        let res = exec_on(rt, client, spec, &clearance, &connect_sid, &xsrf_token, &api_token);
        match &res {
            Ok(r) if r.status != 403 => {
                inner.last_good.store(*idx, Ordering::Relaxed);
                if let Some(tok) = extract_cookie_val(&r.headers, "XSRF-TOKEN") {
                    if let Ok(mut g) = inner.xsrf_token.lock() {
                        *g = tok;
                    }
                }
                if let Some(sid) = extract_cookie_val(&r.headers, "connect.sid") {
                    if let Ok(mut g) = inner.connect_sid.lock() {
                        *g = sid;
                    }
                }
                return res;
            }
            Ok(r) if r.status == 403 => {
                if !is_mobile && spec.url.contains("animecix.tv") && matches!(spec.method, Method::Post) {
                    let bs_spec = Spec {
                        method: Method::Get,
                        url: "https://animecix.tv/secure/bootstrap-data".to_string(),
                        headers: Vec::new(),
                        query: None,
                        json_body: None,
                        timeout_secs: Some(5),
                    };
                    if let Ok(bs_resp) = exec_on(rt, client, &bs_spec, &clearance, &connect_sid, "", "") {
                        if let Some(tok) = extract_cookie_val(&bs_resp.headers, "XSRF-TOKEN") {
                            if let Ok(mut g) = inner.xsrf_token.lock() {
                                *g = tok.clone();
                            }
                            let retry_res = exec_on(rt, client, spec, &clearance, &connect_sid, &tok, &api_token);
                            if let Ok(ref rr) = retry_res {
                                if rr.status != 403 {
                                    inner.last_good.store(*idx, Ordering::Relaxed);
                                    return retry_res;
                                }
                            }
                        }
                    }
                }
                last = Some(res);
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
                .connect_timeout(Duration::from_secs(5))
                .pool_max_idle_per_host(2)
                .pool_idle_timeout(Duration::from_secs(20));
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
                connect_sid: Mutex::new(String::new()),
                xsrf_token: Mutex::new(String::new()),
                api_token: Mutex::new(String::new()),
            }),
        })
    }

    /// Mobil API Bearer tokenini kaydeder.
    pub fn set_api_token(&self, v: &str) {
        let v = v.trim();
        if let Ok(mut g) = self.inner.api_token.lock() {
            *g = v.to_string();
        }
    }

    pub fn api_token(&self) -> String {
        self.inner.api_token.lock().map(|g| g.clone()).unwrap_or_default()
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

    pub fn cf_clearance(&self) -> String {
        self.inner.cf_clearance.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn set_connect_sid(&self, v: &str) {
        let v = v.trim();
        if let Ok(mut g) = self.inner.connect_sid.lock() {
            *g = v.to_string();
        }
    }

    pub fn connect_sid(&self) -> String {
        self.inner.connect_sid.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn set_xsrf_token(&self, v: &str) {
        let v = v.trim();
        if let Ok(mut g) = self.inner.xsrf_token.lock() {
            *g = v.to_string();
        }
    }

    pub fn xsrf_token(&self) -> String {
        self.inner.xsrf_token.lock().map(|g| g.clone()).unwrap_or_default()
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
/// Boş bilet/çerezler, video-host'ları ve çağrıda zaten Cookie varsa None döner.
fn cookie_header_for(
    url: &str,
    existing: &[(String, String)],
    clearance: &str,
    connect_sid: &str,
    xsrf_token: &str,
) -> Option<String> {
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
    let mut parts = Vec::new();
    if !clearance.is_empty() {
        parts.push(format!("cf_clearance={clearance}"));
    }
    if !connect_sid.is_empty() {
        parts.push(format!("connect.sid={connect_sid}"));
    }
    if !xsrf_token.is_empty() {
        parts.push(format!("XSRF-TOKEN={xsrf_token}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::{browser_headers_for, cookie_header_for};

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
    fn clearance_cookie_attached_to_api_only() {
        let v = cookie_header_for("https://animecix.tv/secure/search/x", &[], "BILET123", "", "");
        assert_eq!(v.as_deref(), Some("cf_clearance=BILET123"));
        assert!(cookie_header_for("https://animecix.tv/secure/search/x", &[], "", "", "").is_none());
        assert!(cookie_header_for("https://video.sibnet.ru/v/1/2.mp4", &[], "BILET123", "", "").is_none());
        assert!(cookie_header_for("https://tau-video.xyz/api/video/a", &[], "BILET123", "", "").is_none());
        let with_cookie = vec![("Cookie".to_string(), "a=b".to_string())];
        assert!(cookie_header_for("https://animecix.tv/secure/search/x", &with_cookie, "BILET123", "", "").is_none());

        let with_session = cookie_header_for("https://animecix.tv/secure/history/get-titles", &[], "BILET123", "SID999", "XSRF1");
        assert_eq!(with_session.as_deref(), Some("cf_clearance=BILET123; connect.sid=SID999; XSRF-TOKEN=XSRF1"));
    }
}
