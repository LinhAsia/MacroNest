use std::ffi::c_void;
use std::path::Path;
use anyhow::{Context, Result, bail};
use libloading::{Library, Symbol};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::core::Interface;

type FnNvencCreate = unsafe extern "C" fn(
    device: *mut c_void,
    width: u32,
    height: u32,
    fps: u32,
    bitrate_kbps: u32,
    out_err: *mut i32,
) -> *mut c_void;

type FnNvencRegisterTexture = unsafe extern "C" fn(
    encoder: *mut c_void,
    texture: *mut c_void,
    out_err: *mut i32,
) -> *mut c_void;

type FnNvencEncodeFrame = unsafe extern "C" fn(
    encoder: *mut c_void,
    source_tex: *mut c_void,
    force_idr: i32,
    out_data: *mut *const u8,
    out_size: *mut u32,
) -> i32;

type FnNvencDestroy = unsafe extern "C" fn(encoder: *mut c_void);

pub struct NvencHardwareEncoder {
    _lib: Library,
    encoder: *mut c_void,
    registered_tex: *mut c_void,
    fn_encode_frame: FnNvencEncodeFrame,
    fn_destroy: FnNvencDestroy,
}

// Safety: ID3D11Device and NVENC session pointer are used sequentially on the feeder thread
unsafe impl Send for NvencHardwareEncoder {}

impl NvencHardwareEncoder {
    pub fn new(
        dll_path: &Path,
        device: &ID3D11Device,
        texture: &ID3D11Texture2D,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
    ) -> Result<Self> {
        let lib = unsafe { Library::new(dll_path) }
            .with_context(|| format!("Failed to load NVENC helper DLL: {}", dll_path.display()))?;

        let fn_create: Symbol<FnNvencCreate> = unsafe { lib.get(b"nvenc_create") }?;
        let fn_register: Symbol<FnNvencRegisterTexture> = unsafe { lib.get(b"nvenc_register_texture") }?;
        let fn_encode: Symbol<FnNvencEncodeFrame> = unsafe { lib.get(b"nvenc_encode_frame") }?;
        let fn_destroy: Symbol<FnNvencDestroy> = unsafe { lib.get(b"nvenc_destroy") }?;

        let dev_ptr = device.as_raw() as *mut c_void;
        let mut create_err = 0i32;
        let encoder = unsafe { fn_create(dev_ptr, width, height, fps, bitrate_kbps, &mut create_err) };
        if encoder.is_null() {
            bail!("NVENC hardware session initialization failed with error code: {create_err}");
        }

        let tex_ptr = texture.as_raw() as *mut c_void;
        let mut reg_err = 0i32;
        let registered_tex = unsafe { fn_register(encoder, tex_ptr, &mut reg_err) };
        if registered_tex.is_null() {
            unsafe { fn_destroy(encoder) };
            bail!("Failed to register Direct3D 11 texture with NVENC hardware encoder (code: {reg_err}).");
        }

        Ok(Self {
            fn_encode_frame: *fn_encode,
            fn_destroy: *fn_destroy,
            _lib: lib,
            encoder,
            registered_tex,
        })
    }

    pub fn encode_frame(&self, source_texture: &ID3D11Texture2D, force_idr: bool) -> Result<&'static [u8]> {
        let mut out_data: *const u8 = std::ptr::null();
        let mut out_size = 0u32;
        let tex_ptr = source_texture.as_raw() as *mut c_void;
        let res = unsafe {
            (self.fn_encode_frame)(
                self.encoder,
                tex_ptr,
                if force_idr { 1 } else { 0 },
                &mut out_data,
                &mut out_size,
            )
        };
        if res != 0 {
            bail!("NVENC hardware encode failed with status code: {res}");
        }
        if out_size > 0 && !out_data.is_null() {
            Ok(unsafe { std::slice::from_raw_parts(out_data, out_size as usize) })
        } else {
            Ok(&[])
        }
    }
}

impl Drop for NvencHardwareEncoder {
    fn drop(&mut self) {
        if !self.encoder.is_null() {
            unsafe { (self.fn_destroy)(self.encoder) };
            self.encoder = std::ptr::null_mut();
        }
    }
}