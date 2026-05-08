use anyhow::{anyhow, bail, Result};
use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::ptr;

const JPEG_PIXEL_FORMAT_RGB565_LE: u32 = u32::from_le_bytes(*b"RGBL");
const JPEG_ROTATE_0D: u32 = 0;

#[repr(C)]
#[derive(Copy, Clone)]
struct jpeg_resolution_t {
    width: u16,
    height: u16,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct jpeg_dec_config_t {
    output_type: u32,
    scale: jpeg_resolution_t,
    clipper: jpeg_resolution_t,
    rotate: u32,
    block_enable: bool,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct jpeg_dec_header_info_t {
    width: u16,
    height: u16,
}

#[repr(C)]
struct jpeg_dec_io_t {
    inbuf: *mut u8,
    inbuf_len: i32,
    inbuf_remain: i32,
    outbuf: *mut u8,
    out_size: i32,
}

type JpegDecHandle = *mut c_void;
type JpegError = i32;

const JPEG_ERR_OK: JpegError = 0;

unsafe extern "C" {
    fn jpeg_dec_open(config: *mut jpeg_dec_config_t, jpeg_dec: *mut JpegDecHandle) -> JpegError;
    fn jpeg_dec_parse_header(
        jpeg_dec: JpegDecHandle,
        io: *mut jpeg_dec_io_t,
        out_info: *mut jpeg_dec_header_info_t,
    ) -> JpegError;
    fn jpeg_dec_get_outbuf_len(jpeg_dec: JpegDecHandle, outbuf_len: *mut i32) -> JpegError;
    fn jpeg_dec_process(jpeg_dec: JpegDecHandle, io: *mut jpeg_dec_io_t) -> JpegError;
    fn jpeg_dec_close(jpeg_dec: JpegDecHandle) -> JpegError;
    fn jpeg_calloc_align(size: usize, aligned: i32) -> *mut c_void;
    fn jpeg_free_align(data: *mut c_void);
}

struct DecoderGuard(JpegDecHandle);

impl Drop for DecoderGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = jpeg_dec_close(self.0);
            }
        }
    }
}

struct AlignedBuf(*mut c_void);

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                jpeg_free_align(self.0);
            }
        }
    }
}

pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub rgb565_be: Vec<u16>,
}

pub fn decode_rgb565(jpeg_data: &[u8]) -> Result<DecodedFrame> {
    let mut cfg = jpeg_dec_config_t {
        output_type: JPEG_PIXEL_FORMAT_RGB565_LE,
        scale: jpeg_resolution_t { width: 0, height: 0 },
        clipper: jpeg_resolution_t { width: 0, height: 0 },
        rotate: JPEG_ROTATE_0D,
        block_enable: false,
    };

    let mut handle: JpegDecHandle = ptr::null_mut();
    let open_ret = unsafe { jpeg_dec_open(&mut cfg, &mut handle) };
    if open_ret != JPEG_ERR_OK {
        bail!("jpeg_dec_open failed: {}", open_ret);
    }
    let _decoder = DecoderGuard(handle);

    let mut header = MaybeUninit::<jpeg_dec_header_info_t>::uninit();
    let mut io = jpeg_dec_io_t {
        inbuf: jpeg_data.as_ptr() as *mut u8,
        inbuf_len: jpeg_data.len() as i32,
        inbuf_remain: 0,
        outbuf: ptr::null_mut(),
        out_size: 0,
    };

    let header_ret = unsafe { jpeg_dec_parse_header(handle, &mut io, header.as_mut_ptr()) };
    if header_ret != JPEG_ERR_OK {
        bail!("jpeg_dec_parse_header failed: {}", header_ret);
    }
    let header = unsafe { header.assume_init() };

    let mut out_len: i32 = 0;
    let out_len_ret = unsafe { jpeg_dec_get_outbuf_len(handle, &mut out_len) };
    if out_len_ret != JPEG_ERR_OK || out_len <= 0 {
        bail!("jpeg_dec_get_outbuf_len failed: {}", out_len_ret);
    }

    let outbuf_raw = unsafe { jpeg_calloc_align(out_len as usize, 16) };
    if outbuf_raw.is_null() {
        bail!("jpeg_calloc_align failed");
    }
    let outbuf_guard = AlignedBuf(outbuf_raw);

    io.outbuf = outbuf_guard.0 as *mut u8;
    io.out_size = 0;

    let decode_ret = unsafe { jpeg_dec_process(handle, &mut io) };
    if decode_ret != JPEG_ERR_OK {
        bail!("jpeg_dec_process failed: {}", decode_ret);
    }

    let px_count = (header.width as usize).saturating_mul(header.height as usize);
    let expected_bytes = px_count.saturating_mul(2);
    if io.out_size <= 0 || (io.out_size as usize) < expected_bytes {
        return Err(anyhow!(
            "decoder output too small: out_size={} expected={}",
            io.out_size,
            expected_bytes
        ));
    }

    let out_slice = unsafe { core::slice::from_raw_parts(outbuf_guard.0 as *const u8, expected_bytes) };
    let mut rgb565_be = Vec::with_capacity(px_count);
    for pair in out_slice.chunks_exact(2) {
        rgb565_be.push(u16::from_le_bytes([pair[0], pair[1]]).to_be());
    }

    Ok(DecodedFrame {
        width: header.width as u32,
        height: header.height as u32,
        rgb565_be,
    })
}
