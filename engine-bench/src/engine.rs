//! Safe wrappers over the shim in `shim/shim.cpp`. All of the work is done by the engine's own code.
use anyhow::{Result, bail};
use std::ffi::{CStr, CString, c_char, c_long, c_void};

unsafe extern "C" {
    fn sb_last_error() -> *const c_char;
    fn sb_directives_new(data: *const c_char, len: usize) -> *mut c_void;
    fn sb_directives_free(handle: *mut c_void);
    fn sb_directives_load(handle: *mut c_void) -> c_long;
    fn sb_directives_apply(
        handle: *mut c_void,
        rgba: *const u8,
        width: u32,
        height: u32,
        out_width: *mut u32,
        out_height: *mut u32,
    ) -> *const u8;
    fn sb_directives_apply_sequential(
        handle: *mut c_void,
        rgba: *const u8,
        width: u32,
        height: u32,
        limit: usize,
        out_width: *mut u32,
        out_height: *mut u32,
    ) -> *const u8;
    fn sb_png_decode(data: *const u8, len: usize, width: *mut u32, height: *mut u32) -> *mut u8;
    fn sb_pak_open(path: *const c_char) -> *mut c_void;
    fn sb_pak_close(pak: *mut c_void);
    fn sb_pak_read(pak: *mut c_void, path: *const c_char, len: *mut usize) -> *mut u8;
    fn sb_free(ptr: *mut c_void);
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(sb_last_error()) }
        .to_string_lossy()
        .into_owned()
}

/// An RGBA8 image, rows from the top
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y * self.width + x) as usize * 4;
        self.rgba[i..i + 4].try_into().unwrap()
    }

    /// A copy of the rectangle at (x, y)
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Image {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in y..y + height {
            let start = ((row * self.width + x) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + width as usize * 4]);
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    /// Decodes with the engine's own png reader
    pub fn decode(png: &[u8]) -> Result<Image> {
        let (mut width, mut height) = (0, 0);
        let data = unsafe { sb_png_decode(png.as_ptr(), png.len(), &mut width, &mut height) };
        if data.is_null() {
            bail!("png decode failed: {}", last_error());
        }
        let rgba =
            unsafe { std::slice::from_raw_parts(data, (width * height * 4) as usize) }.to_vec();
        unsafe { sb_free(data.cast()) };
        Ok(Image {
            width,
            height,
            rgba,
        })
    }
}

/// A set of directives, `?a=b?c=d`, as the engine holds them for a drawable
pub struct Directives(*mut c_void);

impl Directives {
    /// Splits the string into operations.
    /// For strings of 1000 characters and more the engine leaves parsing them to `load` (or to the first `apply`).
    pub fn new(directives: &str) -> Result<Directives> {
        let handle = unsafe { sb_directives_new(directives.as_ptr().cast(), directives.len()) };
        if handle.is_null() {
            bail!("directives rejected: {}", last_error());
        }
        Ok(Directives(handle))
    }

    /// Parses all of the operations, which the engine otherwise does on first use. Returns how many there are.
    pub fn load(&self) -> Result<usize> {
        let count = unsafe { sb_directives_load(self.0) };
        if count < 0 {
            bail!("directives failed to parse: {}", last_error());
        }
        Ok(count as usize)
    }

    /// Applies the directives to the image, as for a drawable. The pixels are valid until the next call.
    pub fn apply_in_place(&mut self, image: &Image) -> Result<(u32, u32, &[u8])> {
        let (mut width, mut height) = (0, 0);
        let data = unsafe {
            sb_directives_apply(
                self.0,
                image.rgba.as_ptr(),
                image.width,
                image.height,
                &mut width,
                &mut height,
            )
        };
        if data.is_null() {
            bail!("applying the directives failed: {}", last_error());
        }
        Ok((width, height, unsafe {
            std::slice::from_raw_parts(data, (width * height * 4) as usize)
        }))
    }

    /// Applies one operation after the other, as written, without the engine reordering or combining any of them
    pub fn apply_sequential_in_place(&mut self, image: &Image) -> Result<(u32, u32, &[u8])> {
        self.apply_first(image, usize::MAX)
    }

    /// Applies only the first `limit` operations, one after the other
    pub fn apply_first(&mut self, image: &Image, limit: usize) -> Result<(u32, u32, &[u8])> {
        let (mut width, mut height) = (0, 0);
        let data = unsafe {
            sb_directives_apply_sequential(
                self.0,
                image.rgba.as_ptr(),
                image.width,
                image.height,
                limit,
                &mut width,
                &mut height,
            )
        };
        if data.is_null() {
            bail!("applying the directives failed: {}", last_error());
        }
        Ok((width, height, unsafe {
            std::slice::from_raw_parts(data, (width * height * 4) as usize)
        }))
    }

    pub fn apply(&mut self, image: &Image) -> Result<Image> {
        let (width, height, rgba) = self.apply_in_place(image)?;
        Ok(Image {
            width,
            height,
            rgba: rgba.to_vec(),
        })
    }
}

impl Drop for Directives {
    fn drop(&mut self) {
        unsafe { sb_directives_free(self.0) }
    }
}

/// A packed asset file, `packed.pak`
pub struct Pak(*mut c_void);

impl Pak {
    pub fn open(path: &str) -> Result<Pak> {
        let path = CString::new(path)?;
        let pak = unsafe { sb_pak_open(path.as_ptr()) };
        if pak.is_null() {
            bail!("failed to open the asset pack: {}", last_error());
        }
        Ok(Pak(pak))
    }

    /// The default location of the game's assets
    pub fn open_default() -> Result<Pak> {
        let path = std::env::var("STARBOUND_PAK").unwrap_or_else(|_| {
            format!(
                "{}/.local/share/OpenStarbound/assets/packed.pak",
                std::env::var("HOME").unwrap_or_default()
            )
        });
        Pak::open(&path)
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let c_path = CString::new(path)?;
        let mut len = 0;
        let data = unsafe { sb_pak_read(self.0, c_path.as_ptr(), &mut len) };
        if data.is_null() {
            bail!(
                "failed to read {path} from the asset pack: {}",
                last_error()
            );
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
        unsafe { sb_free(data.cast()) };
        Ok(bytes)
    }
}

impl Drop for Pak {
    fn drop(&mut self) {
        unsafe { sb_pak_close(self.0) }
    }
}
