#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_void, CStr};
use std::os::raw::c_int as Int;
use std::rc::Rc;

use super::ffi::{
    mpv_opengl_fbo, mpv_opengl_init_params, mpv_render_context, mpv_render_param, mpv_update_fn,
    MPV_RENDER_API_TYPE_OPENGL, MPV_RENDER_PARAM_API_TYPE, MPV_RENDER_PARAM_FLIP_Y,
    MPV_RENDER_PARAM_OPENGL_FBO, MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
};
use super::player::MpvApi;

type Callback = Box<dyn Fn() + Send + 'static>;

unsafe extern "C" fn get_proc_addr(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    libc::dlsym(libc::RTLD_DEFAULT, name)
}

unsafe extern "C" fn trampoline(ctx: *mut c_void) {
    let cb = &*(ctx as *const Callback);
    cb();
}

/// libmpv OpenGL render bağlamı. Yalnız GTK ana thread'inde ve GL context
/// current iken kullanılmalıdır.
pub struct RenderContext {
    ctx: *mut mpv_render_context,
    api: Rc<MpvApi>,
    _cb: Option<Box<Callback>>,
}

impl RenderContext {
    pub fn new(api: Rc<MpvApi>, handle: *mut super::ffi::mpv_handle) -> Result<Self, String> {
        let mut init_params = mpv_opengl_init_params {
            get_proc_address: Some(get_proc_addr),
            get_proc_address_ctx: std::ptr::null_mut(),
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
                "mpv_render_context_create başarısız: {}",
                mpv_error(&api, ret)
            ));
        }
        Ok(Self {
            ctx,
            api,
            _cb: None,
        })
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
