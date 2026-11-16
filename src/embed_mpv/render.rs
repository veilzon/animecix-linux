#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_void, CStr};
use std::os::raw::c_int as Int;
use std::rc::Rc;

use libloading::Library;

use super::ffi::{
    mpv_opengl_fbo, mpv_opengl_init_params, mpv_render_context, mpv_render_param, mpv_update_fn,
    MPV_RENDER_API_TYPE_OPENGL, MPV_RENDER_PARAM_API_TYPE, MPV_RENDER_PARAM_FLIP_Y,
    MPV_RENDER_PARAM_OPENGL_FBO, MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
};
use super::player::MpvApi;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlBackend {
    Egl,
    Glx,
}

/// GTK hangi backend'i kullanıyorsa GL sembolleri de oradan çözülmeli.
/// `dlsym(RTLD_DEFAULT)` Wayland/EGL oturumunda GLX sembolü döndürebilir;
/// bu durumda mpv render context'i yanlış fonksiyon tablosuyla kurulur ve
/// ses gelirken video siyah kalır.
pub fn backend_for_env(
    gdk_backend: Option<&str>,
    wayland_display: bool,
    x11_display: bool,
) -> GlBackend {
    if let Some(gdk) = gdk_backend {
        let lower = gdk.to_ascii_lowercase();
        if lower.contains("wayland") {
            return GlBackend::Egl;
        }
        if lower.contains("x11") {
            return GlBackend::Glx;
        }
    }
    if wayland_display {
        GlBackend::Egl
    } else if x11_display {
        GlBackend::Glx
    } else {
        GlBackend::Egl
    }
}

fn detect_backend() -> GlBackend {
    let gdk = std::env::var("GDK_BACKEND").ok();
    backend_for_env(
        gdk.as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("DISPLAY").is_some(),
    )
}

type ProcFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;

struct GlResolver {
    backend: GlBackend,
    _library: Option<Library>,
    get_proc: Option<ProcFn>,
}

impl GlResolver {
    fn new() -> Self {
        let backend = detect_backend();
        let (name, symbol) = match backend {
            GlBackend::Egl => ("libEGL.so.1", b"eglGetProcAddress\0".as_slice()),
            GlBackend::Glx => ("libGL.so.1", b"glXGetProcAddressARB\0".as_slice()),
        };
        let library = unsafe { Library::new(name) }.ok();
        let get_proc = library
            .as_ref()
            .and_then(|lib| unsafe { lib.get::<ProcFn>(symbol).ok().map(|f| *f) });
        Self {
            backend,
            _library: library,
            get_proc,
        }
    }

    fn get(&self, name: *const c_char) -> *mut c_void {
        if let Some(f) = self.get_proc {
            let p = unsafe { f(name) };
            if !p.is_null() {
                return p;
            }
        }
        // Son çare: doğru backend sembolü yoksa eski davranış.
        unsafe { libc::dlsym(libc::RTLD_DEFAULT, name) }
    }

    fn current_fbo(&self) -> c_int {
        const GL_DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
        type GlGetIntegervFn = unsafe extern "C" fn(u32, *mut c_int);
        let sym = self.get(b"glGetIntegerv\0".as_ptr() as *const c_char);
        if sym.is_null() {
            return 0;
        }
        let f: GlGetIntegervFn = unsafe { std::mem::transmute(sym) };
        let mut fbo: c_int = 0;
        unsafe { f(GL_DRAW_FRAMEBUFFER_BINDING, &mut fbo) };
        fbo
    }
}

unsafe extern "C" fn get_proc_addr(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if ctx.is_null() {
        return std::ptr::null_mut();
    }
    let resolver = &*(ctx as *const GlResolver);
    resolver.get(name)
}

type Callback = Box<dyn Fn() + Send + 'static>;

unsafe extern "C" fn trampoline(ctx: *mut c_void) {
    let cb = &*(ctx as *const Callback);
    cb();
}

/// libmpv OpenGL render bağlamı. Yalnız GTK ana thread'inde ve GL context
/// current iken kullanılmalıdır.
pub struct RenderContext {
    ctx: *mut mpv_render_context,
    api: Rc<MpvApi>,
    resolver: Box<GlResolver>,
    _cb: Option<Box<Callback>>,
}

impl RenderContext {
    pub fn new(api: Rc<MpvApi>, handle: *mut super::ffi::mpv_handle) -> Result<Self, String> {
        let mut resolver = Box::new(GlResolver::new());
        let resolver_ptr = (&mut *resolver) as *mut GlResolver as *mut c_void;
        let mut init_params = mpv_opengl_init_params {
            get_proc_address: Some(get_proc_addr),
            get_proc_address_ctx: resolver_ptr,
            extra_exts: std::ptr::null(),
        };
        let mut params = [
            mpv_render_param {
                type_: MPV_RENDER_PARAM_API_TYPE,
                data: MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void,
            },
            mpv_render_param {
                type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init_params as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            },
        ];

        let mut ctx: *mut mpv_render_context = std::ptr::null_mut();
        let ret =
            unsafe { (api.f.mpv_render_context_create)(&mut ctx, handle, params.as_mut_ptr()) };
        if ret != 0 {
            return Err(format!(
                "mpv_render_context_create başarısız: {} (backend={:?})",
                mpv_error(&api, ret),
                resolver.backend
            ));
        }
        Ok(Self {
            ctx,
            api,
            resolver,
            _cb: None,
        })
    }

    pub fn backend(&self) -> GlBackend {
        self.resolver.backend
    }

    pub fn current_fbo(&self) -> c_int {
        self.resolver.current_fbo()
    }

    pub fn set_update_callback<F: Fn() + Send + 'static>(&mut self, cb: F) {
        let boxed: Box<Callback> = Box::new(Box::new(cb));
        let ptr = Box::into_raw(boxed);
        unsafe {
            (self.api.f.mpv_render_context_set_update_callback)(
                self.ctx,
                Some(trampoline as mpv_update_fn),
                ptr as *mut c_void,
            );
        }
        self._cb = Some(unsafe { Box::from_raw(ptr) });
    }

    pub fn render(&self, fbo: Int, w: Int, h: Int, flip_y: bool) -> Result<(), String> {
        let mut fbo_params = mpv_opengl_fbo {
            fbo,
            w,
            h,
            internal_format: 0,
        };
        let mut flip: c_int = flip_y as c_int;
        let mut params = [
            mpv_render_param {
                type_: MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut fbo_params as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip as *mut _ as *mut c_void,
            },
            mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            },
        ];
        let ret = unsafe { (self.api.f.mpv_render_context_render)(self.ctx, params.as_mut_ptr()) };
        if ret != 0 {
            return Err(format!(
                "mpv_render_context_render başarısız: {}",
                mpv_error(&self.api, ret)
            ));
        }
        Ok(())
    }

    pub fn report_swap(&self) {
        unsafe { (self.api.f.mpv_render_context_report_swap)(self.ctx) };
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        if self.ctx.is_null() {
            return;
        }
        unsafe {
            (self.api.f.mpv_render_context_set_update_callback)(
                self.ctx,
                None,
                std::ptr::null_mut(),
            );
            (self.api.f.mpv_render_context_free)(self.ctx);
        }
        self.ctx = std::ptr::null_mut();
    }
}

fn mpv_error(api: &MpvApi, code: c_int) -> String {
    let p = unsafe { (api.f.mpv_error_string)(code) };
    if p.is_null() {
        return format!("kod {code}");
    }
    unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_uses_egl_x11_uses_glx() {
        assert_eq!(backend_for_env(None, true, true), GlBackend::Egl);
        assert_eq!(backend_for_env(None, false, true), GlBackend::Glx);
        assert_eq!(
            backend_for_env(Some("wayland"), false, true),
            GlBackend::Egl
        );
        assert_eq!(backend_for_env(Some("x11"), true, true), GlBackend::Glx);
        assert_eq!(
            backend_for_env(Some("wayland,x11"), true, true),
            GlBackend::Egl
        );
    }
}
