//! 截鼠标附近一小块屏幕，用系统自带的文字识别（`Windows.Media.Ocr`）找鼠标下的词。
//! 程序不给辅助接口文字范围（自绘界面、图片、部分 PDF）时用；识别语言跟随系统已装的识别包。

use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// 截图区域是鼠标左右各多宽、上下各多高（物理像素）。
const HALF_WIDTH: i32 = 200;
const HALF_HEIGHT: i32 = 40;

/// 识别前放大几倍：界面小字放大后认得准。
const SCALE: i32 = 2;

/// 鼠标 `(x, y)` 下识别出的词及其屏幕位置。
pub(super) fn word_at(x: i32, y: i32) -> Option<(String, RECT)> {
    let area = region(x, y);
    let (width, height) = (area.right - area.left, area.bottom - area.top);
    let pixels = capture(&area)?;
    let pixels = upscale(&pixels, width, height);
    let (cx, cy) = ((x - area.left) * SCALE, (y - area.top) * SCALE);
    let words = recognize(&pixels, width * SCALE, height * SCALE)
        .inspect_err(|error| tracing::debug!(%error, "hover OCR failed"))
        .ok()?;
    words.into_iter().find_map(|(text, rect)| {
        let inside = (rect.left..rect.right).contains(&cx) && (rect.top..rect.bottom).contains(&cy);
        let word = text.trim_matches(|c: char| !c.is_alphanumeric());
        (inside && !word.is_empty()).then(|| {
            let screen = RECT {
                left: area.left + rect.left / SCALE,
                top: area.top + rect.top / SCALE,
                right: area.left + rect.right / SCALE,
                bottom: area.top + rect.bottom / SCALE,
            };
            (word.to_owned(), screen)
        })
    })
}

/// 鼠标周围的截图区域，不超出整个桌面（多屏时坐标可以是负数）。
fn region(x: i32, y: i32) -> RECT {
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    RECT {
        left: (x - HALF_WIDTH).max(left),
        top: (y - HALF_HEIGHT).max(top),
        right: (x + HALF_WIDTH).min(left + width),
        bottom: (y + HALF_HEIGHT).min(top + height),
    }
}

/// 截屏成自上而下的 BGRA 像素；透明度补成不透明，识别器才不把它当黑底。
fn capture(area: &RECT) -> Option<Vec<u8>> {
    let (width, height) = (area.right - area.left, area.bottom - area.top);
    if width <= 0 || height <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(memory, bitmap.into());
        let copied = BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            area.left,
            area.top,
            SRCCOPY,
        );
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let lines = GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(memory, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        if copied.is_err() || lines == 0 {
            return None;
        }
        for alpha in pixels.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
        Some(pixels)
    }
}

/// 最近邻放大 [`SCALE`] 倍。
fn upscale(pixels: &[u8], width: i32, height: i32) -> Vec<u8> {
    let (width, height, scale) = (width as usize, height as usize, SCALE as usize);
    let mut out = vec![0u8; pixels.len() * scale * scale];
    for y in 0..height * scale {
        for x in 0..width * scale {
            let from = ((y / scale) * width + x / scale) * 4;
            let to = (y * width * scale + x) * 4;
            out[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
        }
    }
    out
}

/// 识别一张 BGRA 图，给出每个词及其在图上的框。
fn recognize(pixels: &[u8], width: i32, height: i32) -> windows::core::Result<Vec<(String, RECT)>> {
    let buffer = CryptographicBuffer::CreateFromByteArray(pixels)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width,
        height,
        BitmapAlphaMode::Premultiplied,
    )?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let mut words = Vec::new();
    for line in result.Lines()? {
        for word in line.Words()? {
            let rect = word.BoundingRect()?;
            words.push((
                word.Text()?.to_string(),
                RECT {
                    left: rect.X as i32,
                    top: rect.Y as i32,
                    right: (rect.X + rect.Width) as i32,
                    bottom: (rect.Y + rect.Height) as i32,
                },
            ));
        }
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::{SCALE, upscale};

    #[test]
    fn upscaling_repeats_each_pixel() {
        let pixels = [1, 2, 3, 4, 5, 6, 7, 8];
        let out = upscale(&pixels, 2, 1);
        assert_eq!(out.len(), pixels.len() * (SCALE * SCALE) as usize);
        assert_eq!(&out[..8], &[1, 2, 3, 4, 1, 2, 3, 4]);
        assert_eq!(&out[16..24], &[1, 2, 3, 4, 1, 2, 3, 4]);
    }
}
