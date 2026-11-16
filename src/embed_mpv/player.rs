use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::rc::Rc;

use super::ffi::*;
use super::loader::LoadedLibmpv;
use super::render::RenderContext;

pub struct MpvApi {
    pub loaded: LoadedLibmpv,
    pub f: MpvFunctions,
}

impl MpvApi {
    pub fn from_loaded(loaded: LoadedLibmpv) -> Result<Self, String> {
        let lib = &loaded.library;
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let s = unsafe { lib.get::<$ty>(concat!($name, "\0").as_bytes()) }
                    .map_err(|e| format!("{name}: {e}", name = $name))?;
                *s
            }};
        }

        let f = MpvFunctions {
            mpv_create: symbol!("mpv_create", unsafe extern "C" fn() -> *mut mpv_handle),
            mpv_initialize: symbol!(
                "mpv_initialize",
                unsafe extern "C" fn(*mut mpv_handle) -> c_int
            ),
            mpv_terminate_destroy: symbol!(
                "mpv_terminate_destroy",
                unsafe extern "C" fn(*mut mpv_handle)
            ),
            mpv_error_string: symbol!(
                "mpv_error_string",
                unsafe extern "C" fn(c_int) -> *const c_char
            ),
            mpv_set_option_string: symbol!(
                "mpv_set_option_string",
                unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int
            ),
            mpv_set_property: symbol!(
                "mpv_set_property",
                unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int
            ),
            mpv_get_property: symbol!(
                "mpv_get_property",
                unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int
            ),
            mpv_command: symbol!(
                "mpv_command",
                unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char) -> c_int
            ),
            mpv_free: symbol!("mpv_free", unsafe extern "C" fn(*mut c_void)),
            mpv_render_context_create: symbol!(
                "mpv_render_context_create",
                unsafe extern "C" fn(
                    *mut *mut mpv_render_context,
                    *mut mpv_handle,
                    *mut mpv_render_param,
                ) -> c_int
            ),
            mpv_render_context_render: symbol!(
                "mpv_render_context_render",
                unsafe extern "C" fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int
            ),
            mpv_render_context_report_swap: symbol!(
                "mpv_render_context_report_swap",
                unsafe extern "C" fn(*mut mpv_render_context)
            ),
            mpv_render_context_free: symbol!(
                "mpv_render_context_free",
                unsafe extern "C" fn(*mut mpv_render_context)
            ),
            mpv_render_context_set_update_callback: symbol!(
                "mpv_render_context_set_update_callback",
                unsafe extern "C" fn(*mut mpv_render_context, Option<mpv_update_fn>, *mut c_void)
            ),
        };
        Ok(Self { loaded, f })
    }

    pub fn error(&self, code: c_int) -> String {
        error_string(self, code)
    }
}

pub struct MpvEmbed {
    api: Rc<MpvApi>,
    ctx: *mut mpv_handle,
}

impl MpvEmbed {
    pub fn new(
        api: Rc<MpvApi>,
        opts: &[(&str, &str)],
        start_pos: Option<f64>,
    ) -> Result<Self, String> {
        unsafe {
            libc::setlocale(libc::LC_NUMERIC, b"C\0".as_ptr() as *const c_char);
        }
        let ctx = unsafe { (api.f.mpv_create)() };
        if ctx.is_null() {
            return Err("mpv_create NULL döndü".into());
        }

        let mut guard = HandleGuard {
            api: api.clone(),
            ctx,
            armed: true,
        };

        set_opt_required(&api, ctx, "vo", "libmpv")?;
        // Gömülü render'da NVIDIA/CUDA/VDPAU interop'u siyah video üretiyor.
        // Ses gelip görüntü gelmiyorsa kök neden hwdec'tir; yazılım decode kullan.
        set_opt_log(&api, ctx, "hwdec", "no");
        set_opt_log(&api, ctx, "hr-seek", "yes");
        set_opt_log(&api, ctx, "keep-open", "yes");
        set_opt_log(&api, ctx, "cache", "yes");
        set_opt_log(&api, ctx, "network-timeout", "10");
        set_opt_log(
            &api,
            ctx,
            "user-agent",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        );
        set_opt_log(&api, ctx, "osc", "no");
        set_opt_log(&api, ctx, "msg-level", "all=warn");
        for (k, v) in opts {
            set_opt_log(&api, ctx, k, v);
        }
        if let Some(pos) = start_pos_option(start_pos) {
            set_opt_log(&api, ctx, "start", &format!("{pos:.1}"));
        }

        let ret = unsafe { (api.f.mpv_initialize)(ctx) };
        if ret != 0 {
            return Err(format!("mpv_initialize başarısız: {}", api.error(ret)));
        }
        set_prop_double(&api, ctx, "volume", 100.0);
        eprintln!(
            "[EMBED] vo={:?} hwdec={:?}",
            get_string(&api, ctx, "vo"),
            get_string(&api, ctx, "hwdec")
        );
        guard.armed = false;

        eprintln!(
            "[EMBED] mpv hazır (API {}, path={})",
            super::loader::version_label(api.loaded.api_version),
            api.loaded.path
        );
        Ok(Self { api, ctx })
    }

    pub fn create_render_context(&self) -> Result<RenderContext, String> {
        RenderContext::new(self.api.clone(), self.ctx)
    }

    pub fn load_url(&self, url: &str) -> Result<(), String> {
        self.command(&["loadfile", url, "replace"])?;
        self.set_pause(true);
        Ok(())
    }

    pub fn set_http_headers(&self, referer: &str) {
        set_prop_str(
            &self.api,
            self.ctx,
            "http-header-fields",
            &format!("Referer: {referer}\nAccept: */*"),
        );
    }

    pub fn toggle_pause(&self) {
        let _ = self.command(&["cycle", "pause"]);
    }

    pub fn set_pause(&self, p: bool) {
        set_prop_flag(&self.api, self.ctx, "pause", p);
    }

    pub fn seek_abs(&self, secs: f64) {
        let _ = self.command(&["seek", &format!("{secs:.1}"), "absolute"]);
    }

    pub fn seek_rel(&self, secs: f64) {
        let _ = self.command(&["seek", &format!("{secs:.0}")]);
    }

    pub fn stop(&self) {
        let _ = self.command(&["stop"]);
    }

    pub fn pos(&self) -> f64 {
        get_double(&self.api, self.ctx, "time-pos")
    }

    pub fn dur(&self) -> f64 {
        get_double(&self.api, self.ctx, "duration")
    }

    pub fn volume(&self) -> f64 {
        get_double(&self.api, self.ctx, "volume")
    }

    pub fn set_volume(&self, v: f64) {
        set_prop_double(&self.api, self.ctx, "volume", v.clamp(0.0, 100.0));
    }

    pub fn muted(&self) -> bool {
        get_flag(&self.api, self.ctx, "mute", false)
    }

    pub fn set_mute(&self, m: bool) {
        set_prop_flag(&self.api, self.ctx, "mute", m);
    }

    pub fn set_speed(&self, v: f64) {
        set_prop_double(&self.api, self.ctx, "speed", v.clamp(0.25, 4.0));
    }

    pub fn video_height(&self) -> Option<i64> {
        get_i64(&self.api, self.ctx, "video-params/h").filter(|h| *h > 0)
    }

    pub fn panscan(&self) -> f64 {
        get_double(&self.api, self.ctx, "panscan")
    }

    pub fn set_panscan(&self, v: f64) {
        set_prop_double(&self.api, self.ctx, "panscan", v.clamp(0.0, 1.0));
    }

    pub fn paused(&self) -> bool {
        get_flag(&self.api, self.ctx, "pause", true)
    }

    pub fn idle(&self) -> bool {
        get_flag(&self.api, self.ctx, "idle-active", true)
    }

    pub fn eof(&self) -> bool {
        get_flag(&self.api, self.ctx, "eof-reached", false)
    }

    pub fn screenshot_to_file(&self, path: &str) -> Result<(), String> {
        self.command(&["screenshot-to-file", path])
    }

    pub fn video_info(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let (Some(w), Some(h)) = (
            get_i64(&self.api, self.ctx, "video-params/w"),
            get_i64(&self.api, self.ctx, "video-params/h"),
        ) {
            if w > 0 && h > 0 {
                out.push(("Çözünürlük".into(), format!("{w}×{h}")));
            }
        }
        let mut fps = get_double(&self.api, self.ctx, "video-params/fps");
        if fps <= 0.0 {
            fps = get_double(&self.api, self.ctx, "container-fps");
        }
        if fps > 0.0 {
            out.push(("FPS".into(), format!("{fps:.2}")));
        }
        if let Some(c) = get_string(&self.api, self.ctx, "video-codec") {
            if !c.is_empty() {
                out.push(("Video".into(), c));
            }
        }
        if let Some(c) = get_string(&self.api, self.ctx, "audio-codec") {
            if !c.is_empty() {
                out.push(("Ses".into(), c));
            }
        }
        let dur = self.dur();
        if dur > 0.0 {
            out.push(("Süre".into(), super::fmt_time(dur)));
        }
        if let Some(sz) = get_i64(&self.api, self.ctx, "file-size") {
            if sz > 0 {
                let mb = sz as f64 / 1048576.0;
                out.push((
                    "Boyut".into(),
                    if mb >= 1024.0 {
                        format!("{:.2} GB", mb / 1024.0)
                    } else {
                        format!("{mb:.1} MB")
                    },
                ));
            }
        }
        out
    }

    fn command(&self, args: &[&str]) -> Result<(), String> {
        let cs: Vec<CString> = args
            .iter()
            .map(|s| CString::new(*s).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        let mut ptrs: Vec<*const c_char> = cs.iter().map(|s| s.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        let ret = unsafe { (self.api.f.mpv_command)(self.ctx, ptrs.as_mut_ptr()) };
        if ret != 0 {
            return Err(format!(
                "mpv_command başarısız ({}): {}",
                args.first().copied().unwrap_or(""),
                self.api.error(ret)
            ));
        }
        Ok(())
    }
}

impl Drop for MpvEmbed {
    fn drop(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        // RenderContext çağıranda bunun önünde düşürülmüş olmalı.
        unsafe { (self.api.f.mpv_terminate_destroy)(self.ctx) };
        self.ctx = std::ptr::null_mut();
        eprintln!("[EMBED] mpv çekirdeği yok edildi");
    }
}

struct HandleGuard {
    api: Rc<MpvApi>,
    ctx: *mut mpv_handle,
    armed: bool,
}

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if self.armed && !self.ctx.is_null() {
            unsafe { (self.api.f.mpv_terminate_destroy)(self.ctx) };
        }
    }
}

fn error_string(api: &MpvApi, code: c_int) -> String {
    let p = unsafe { (api.f.mpv_error_string)(code) };
    if p.is_null() {
        return format!("kod {code}");
    }
    unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
}

fn set_opt_required(
    api: &MpvApi,
    ctx: *mut mpv_handle,
    name: &str,
    val: &str,
) -> Result<(), String> {
    let n = CString::new(name).map_err(|e| e.to_string())?;
    let v = CString::new(val).map_err(|e| e.to_string())?;
    let ret = unsafe { (api.f.mpv_set_option_string)(ctx, n.as_ptr(), v.as_ptr()) };
    if ret != 0 {
        return Err(format!(
            "mpv option {name}={val} başarısız: {}",
            api.error(ret)
        ));
    }
    Ok(())
}

fn set_opt_log(api: &MpvApi, ctx: *mut mpv_handle, name: &str, val: &str) {
    if let Err(e) = set_opt_required(api, ctx, name, val) {
        eprintln!("[EMBED] {e}");
    }
}

fn set_prop_str(api: &MpvApi, ctx: *mut mpv_handle, name: &str, val: &str) {
    let (Ok(n), Ok(v)) = (CString::new(name), CString::new(val)) else {
        return;
    };
    let mut ptr = v.as_ptr() as *mut c_void;
    unsafe {
        (api.f.mpv_set_property)(
            ctx,
            n.as_ptr(),
            MPV_FORMAT_STRING,
            &mut ptr as *mut _ as *mut c_void,
        );
    }
}

fn set_prop_flag(api: &MpvApi, ctx: *mut mpv_handle, name: &str, val: bool) {
    let Ok(n) = CString::new(name) else {
        return;
    };
    let mut v: c_int = val as c_int;
    unsafe {
        (api.f.mpv_set_property)(
            ctx,
            n.as_ptr(),
            MPV_FORMAT_FLAG,
            &mut v as *mut _ as *mut c_void,
        );
    }
}

fn set_prop_double(api: &MpvApi, ctx: *mut mpv_handle, name: &str, val: f64) {
    let Ok(n) = CString::new(name) else {
        return;
    };
    let mut v: c_double = val;
    unsafe {
        (api.f.mpv_set_property)(
            ctx,
            n.as_ptr(),
            MPV_FORMAT_DOUBLE,
            &mut v as *mut _ as *mut c_void,
        );
    }
}

fn get_double(api: &MpvApi, ctx: *mut mpv_handle, name: &str) -> f64 {
    let Ok(n) = CString::new(name) else {
        return 0.0;
    };
    let mut v: c_double = 0.0;
    let ret = unsafe {
        (api.f.mpv_get_property)(
            ctx,
            n.as_ptr(),
            MPV_FORMAT_DOUBLE,
            &mut v as *mut _ as *mut c_void,
        )
    };
    if ret == 0 {
        v
    } else {
        0.0
    }
}

fn get_flag(api: &MpvApi, ctx: *mut mpv_handle, name: &str, def: bool) -> bool {
    let Ok(n) = CString::new(name) else {
        return def;
    };
    let mut v: c_int = 0;
    let ret = unsafe {
        (api.f.mpv_get_property)(
            ctx,
            n.as_ptr(),
            MPV_FORMAT_FLAG,
            &mut v as *mut _ as *mut c_void,
        )
    };
    if ret == 0 {
        v != 0
    } else {
        def
    }
}

fn get_i64(api: &MpvApi, ctx: *mut mpv_handle, name: &str) -> Option<i64> {
    let Ok(n) = CString::new(name) else {
        return None;
    };
    let mut v: i64 = 0;
    let ret = unsafe {
        (api.f.mpv_get_property)(
            ctx,
            n.as_ptr(),
            MPV_FORMAT_INT64,
            &mut v as *mut _ as *mut c_void,
        )
    };
    if ret == 0 {
        Some(v)
    } else {
        None
    }
}

fn get_string(api: &MpvApi, ctx: *mut mpv_handle, name: &str) -> Option<String> {
    let Ok(n) = CString::new(name) else {
        return None;
    };
    let mut ptr: *mut c_char = std::ptr::null_mut();
    let ret = unsafe {
        (api.f.mpv_get_property)(
            ctx,
            n.as_ptr(),
            MPV_FORMAT_STRING,
            &mut ptr as *mut _ as *mut c_void,
        )
    };
    if ret != 0 || ptr.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() };
    unsafe { (api.f.mpv_free)(ptr as *mut c_void) };
    Some(s)
}

pub(crate) fn options_from_cli(args: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for a in args {
        let t = a.strip_prefix("--").unwrap_or(a);
        if let Some((k, v)) = t.split_once('=') {
            out.push((k.to_string(), v.to_string()));
        } else if let Some(k) = t.strip_prefix("no-") {
            out.push((k.to_string(), "no".to_string()));
        } else if !t.is_empty() {
            out.push((t.to_string(), "yes".to_string()));
        }
    }
    out
}

pub(crate) fn start_pos_option(saved: Option<f64>) -> Option<f64> {
    saved.filter(|s| *s >= 5.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpv_cli_args_become_options() {
        let args = vec![
            "--scale=ewa_lanczossharp".to_string(),
            "--glsl-shaders=/tmp/x.glsl".to_string(),
            "--no-osc".to_string(),
        ];
        let opts = options_from_cli(&args);
        assert!(opts.iter().any(|(k, _)| k == "glsl-shaders"));
        assert!(opts.iter().all(|(k, _)| !k.starts_with("--")));
        assert_eq!(
            opts.iter()
                .find(|(k, _)| k == "osc")
                .map(|(_, v)| v.as_str()),
            Some("no")
        );
    }

    #[test]
    fn resume_only_after_five_seconds() {
        assert_eq!(start_pos_option(None), None);
        assert_eq!(start_pos_option(Some(4.9)), None);
        assert_eq!(start_pos_option(Some(5.0)), Some(5.0));
    }
}
