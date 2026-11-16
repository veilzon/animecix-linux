#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};

#[repr(C)]
pub struct mpv_handle {
    _private: [u8; 0],
}

#[repr(C)]
pub struct mpv_render_context {
    _private: [u8; 0],
}

pub type mpv_update_fn = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
pub struct mpv_opengl_init_params {
    pub get_proc_address: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void>,
    pub get_proc_address_ctx: *mut c_void,
    pub extra_exts: *const *const c_char,
}

#[repr(C)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

#[repr(C)]
pub struct mpv_render_param {
    pub type_: c_int,
    pub data: *mut c_void,
}

pub const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
pub const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
pub const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
pub const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;

pub const MPV_FORMAT_STRING: c_int = 1;
pub const MPV_FORMAT_FLAG: c_int = 3;
pub const MPV_FORMAT_INT64: c_int = 4;
pub const MPV_FORMAT_DOUBLE: c_int = 5;

pub const MPV_RENDER_API_TYPE_OPENGL: &[u8] = b"opengl\0";

#[derive(Clone, Copy)]
pub struct MpvFunctions {
    pub mpv_create: unsafe extern "C" fn() -> *mut mpv_handle,
    pub mpv_initialize: unsafe extern "C" fn(*mut mpv_handle) -> c_int,
    pub mpv_terminate_destroy: unsafe extern "C" fn(*mut mpv_handle),
    pub mpv_error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub mpv_set_option_string:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int,
    pub mpv_set_property:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub mpv_get_property:
        unsafe extern "C" fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub mpv_command: unsafe extern "C" fn(*mut mpv_handle, *mut *const c_char) -> c_int,
    pub mpv_free: unsafe extern "C" fn(*mut c_void),
    pub mpv_render_context_create: unsafe extern "C" fn(
        *mut *mut mpv_render_context,
        *mut mpv_handle,
        *mut mpv_render_param,
    ) -> c_int,
    pub mpv_render_context_render:
        unsafe extern "C" fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int,
    pub mpv_render_context_report_swap: unsafe extern "C" fn(*mut mpv_render_context),
    pub mpv_render_context_free: unsafe extern "C" fn(*mut mpv_render_context),
    pub mpv_render_context_set_update_callback:
        unsafe extern "C" fn(*mut mpv_render_context, Option<mpv_update_fn>, *mut c_void),
}
