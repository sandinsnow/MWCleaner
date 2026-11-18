use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::{div, img, px, AnyElement, IntoElement, RenderImage, Styled};
use image::{Frame, RgbaImage};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{
    SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

use crate::core::startup::{exe_path, expand_env};
use crate::ui::theme;

fn cache() -> &'static Mutex<HashMap<String, Option<Arc<RenderImage>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<RenderImage>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn binary_candidates(raw: &str) -> Vec<String> {
    let cmd = raw.trim();
    let mut out = Vec::new();
    if let Some(rest) = cmd.strip_prefix('"') {
        if let Some(p) = rest.split('"').next() {
            out.push(p.to_string());
        }
    }
    if let Some(pos) = cmd.to_ascii_lowercase().find(".exe") {
        out.push(cmd[..pos + 4].trim_matches('"').to_string());
    }
    if let Some(first) = cmd.split_whitespace().next() {
        out.push(first.trim_matches('"').to_string());
    }
    out.push(cmd.to_string());
    out
}

fn normalize_binary(raw: &str) -> Option<PathBuf> {
    for c in binary_candidates(raw) {
        let c = if let Some(rest) = c.strip_prefix("\\SystemRoot\\") {
            format!("%SystemRoot%\\{rest}")
        } else {
            c
        };
        let p = PathBuf::from(expand_env(&c));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

unsafe fn hicon_to_image(hicon: HICON) -> Option<Arc<RenderImage>> {
    let mut info = ICONINFO::default();
    GetIconInfo(hicon, &mut info).ok()?;
    let mut bm = BITMAP::default();
    let ok = GetObjectW(
        info.hbmColor,
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut _),
    ) > 0;
    let (w, h) = if ok && bm.bmWidth > 0 && bm.bmHeight > 0 {
        (bm.bmWidth as u32, bm.bmHeight.unsigned_abs())
    } else {
        (32, 32)
    };
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let hdc = windows::Win32::Graphics::Gdi::GetDC(HWND::default());
    let got = GetDIBits(
        hdc,
        info.hbmColor,
        0,
        h,
        Some(px.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );
    let mut mask = vec![0u8; (((w + 31) / 32) * 4 * h) as usize];
    let mut mbmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let got_mask = GetDIBits(
        hdc,
        info.hbmMask,
        0,
        h,
        Some(mask.as_mut_ptr() as *mut _),
        &mut mbmi,
        DIB_RGB_COLORS,
    );
    windows::Win32::Graphics::Gdi::ReleaseDC(HWND::default(), hdc);
    let _ = windows::Win32::Graphics::Gdi::DeleteObject(info.hbmColor);
    let _ = windows::Win32::Graphics::Gdi::DeleteObject(info.hbmMask);
    if got == 0 {
        return None;
    }
    let has_alpha = px.chunks(4).any(|p| p[3] != 0);
    let stride = ((w + 31) / 32) * 4;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let b = px[i];
            let g = px[i + 1];
            let r = px[i + 2];
            let mut a = px[i + 3];
            if !has_alpha {
                a = if got_mask != 0 {
                    let bit = mask[(y * stride + x / 8) as usize] & (0x80 >> (x % 8));
                    if bit != 0 { 0 } else { 255 }
                } else if r | g | b != 0 {
                    255
                } else {
                    0
                };
            } else if a > 0 && a < 255 {
                // 图标位图为预乘 alpha，gpui 需要直 alpha
                let un = |c: u8| ((c as u32 * 255) / a as u32).min(255) as u8;
                // gpui Windows 表面按 BGRA 字节序取样
                rgba[i] = un(b);
                rgba[i + 1] = un(g);
                rgba[i + 2] = un(r);
                rgba[i + 3] = a;
                continue;
            }
            rgba[i] = b;
            rgba[i + 1] = g;
            rgba[i + 2] = r;
            rgba[i + 3] = a;
        }
    }
    let buf = RgbaImage::from_raw(w, h, rgba)?;
    Some(Arc::new(RenderImage::new(vec![Frame::new(buf)])))
}

fn extract(path: &PathBuf) -> Option<Arc<RenderImage>> {
    unsafe {
        let mut shfi = SHFILEINFOW::default();
        let r = SHGetFileInfoW(
            PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if r == 0 || shfi.hIcon.is_invalid() {
            return None;
        }
        let img = hicon_to_image(shfi.hIcon);
        let _ = DestroyIcon(shfi.hIcon);
        img
    }
}

fn lookup(path: PathBuf) -> Option<Arc<RenderImage>> {
    let key = path.to_string_lossy().to_lowercase();
    let mut map = cache().lock().ok()?;
    if let Some(v) = map.get(&key) {
        return v.clone();
    }
    let v = extract(&path);
    map.insert(key, v.clone());
    v
}

pub fn for_command(command: &str) -> Option<Arc<RenderImage>> {
    lookup(exe_path(command)?)
}

pub fn for_binary(raw: &str) -> Option<Arc<RenderImage>> {
    lookup(normalize_binary(raw)?)
}

pub fn icon_or_placeholder(icon: &Option<Arc<RenderImage>>) -> AnyElement {
    match icon {
        Some(im) => img(im.clone())
            .size(px(28.))
            .flex_none()
            .into_any_element(),
        None => div()
            .size(px(28.))
            .flex_none()
            .rounded_md()
            .bg(theme::track())
            .into_any_element(),
    }
}