// AnimeciX okul-proxy'si (Faz 1): yalnız API JSON + kapak + kasa geçer.
// Video akışı (sibnet/streamtape/CDN mp4) BİLEREK yok: Referer hotlink
// koruması + Range/206 zorunluluğu proxy'de kırılır, mpv seek edemez.
// Kullanım: GET https://<worker>/animecix.tv/secure/search/naruto?limit=20

const ALLOW = [
  { host: "animecix.tv", paths: ["/secure/", "/"] },
  { host: "tau-video.xyz", paths: ["/api/"] },
  { host: "image.tmdb.org", paths: ["/"] },
  { host: "raw.githubusercontent.com", paths: ["/veilzon/"] },
];

const HOP_BY_HOP = new Set([
  "connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
  "te", "trailer", "transfer-encoding", "upgrade", "host", "content-length",
]);

function allowedTarget(host, path) {
  const rule = ALLOW.find((r) => r.host === host.toLowerCase());
  if (!rule) return false;
  return rule.paths.some((p) => path === p.slice(0, -1) || path.startsWith(p));
}

export default {
  async fetch(req) {
    const url = new URL(req.url);
    if (req.method !== "GET" && req.method !== "HEAD") {
      return new Response("method not allowed", { status: 405 });
    }
    // /{host}/{path...}?query — ilk segment hedef host.
    const m = url.pathname.match(/^\/([^/]+)(\/.*)?$/);
    if (!m) return new Response("bad request", { status: 400 });
    const host = m[1].toLowerCase();
    const path = m[2] || "/";
    if (!allowedTarget(host, path)) return new Response("forbidden", { status: 403 });

    const target = new URL(`https://${host}${path}${url.search}`);
    const fwd = new Headers();
    req.headers.forEach((v, k) => {
      if (!HOP_BY_HOP.has(k.toLowerCase())) fwd.append(k, v);
    });
    fwd.set("Host", target.host);

    let upstream;
    try {
      upstream = await fetch(target.toString(), {
        method: req.method,
        headers: fwd,
        redirect: "follow",
      });
    } catch (e) {
      return new Response("upstream error", { status: 502 });
    }
    const out = new Headers();
    upstream.headers.forEach((v, k) => {
      if (!HOP_BY_HOP.has(k.toLowerCase())) out.append(k, v);
    });
    out.set("Cache-Control", "no-store");
    // Redirect takibi yapan istemciler (embed bulma) için nihai adres.
    try {
      out.set("X-Final-Url", upstream.url);
    } catch (e) { /* yoksay */ }
    return new Response(upstream.body, { status: upstream.status, headers: out });
  },
};
