// src/main.rs
#![no_std]
#![no_main]
#![feature(offset_of)]

use core::arch::asm;
use core::mem::{offset_of, size_of};
use core::panic::PanicInfo;
use core::ptr::null_mut;

type EfiVoid = u8;
type EfiHandle = u64;
type Result<T> = core::result::Result<T, &'static str>;

#[inline(always)]
fn hlt_loop() -> ! {
    loop {
        unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) }
    }
}

// ===== GUID / Status 等 =====

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct EfiGuid {
    data0: u32,
    data1: u16,
    data2: u16,
    data3: [u8; 8],
}

const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data0: 0x9042a9de,
    data1: 0x23dc,
    data2: 0x4a38,
    data3: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};

#[repr(u64)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[must_use]
enum EfiStatus {
    Success = 0,
}

// ===== System Table / Boot Services =====

#[repr(C)]
struct EfiBootServicesTable {
    _reserved0: [u64; 40],
    // UEFI(x86_64) は Microsoft x64 ABI
    locate_protocol: extern "win64" fn(
        protocol: *const EfiGuid,
        registration: *const EfiVoid,
        interface: *mut *mut EfiVoid,
    ) -> EfiStatus,
}
// 仕様どおりのオフセット確認（壊れていないかをビルド時に検査）
const _: () = assert!(offset_of!(EfiBootServicesTable, locate_protocol) == 320);

#[repr(C)]
struct EfiSystemTable {
    _reserved0: [u64; 12],
    boot_services: &'static EfiBootServicesTable,
}
const _: () = assert!(offset_of!(EfiSystemTable, boot_services) == 96);

// ===== Graphics Output Protocol（必要最小限の定義）=====

#[repr(C)]
#[derive(Debug)]
struct EfiGraphicsOutputProtocolPixelInfo {
    version: u32,
    horizontal_resolution: u32,
    vertical_resolution: u32,
    _padding0: [u32; 5],
    pixels_per_scan_line: u32,
}
// 本書の最小定義に合わせたサイズ検証
const _: () = assert!(size_of::<EfiGraphicsOutputProtocolPixelInfo>() == 36);

#[repr(C)]
#[derive(Debug)]
struct EfiGraphicsOutputProtocolMode<'a> {
    max_mode: u32,
    mode: u32,
    info: &'a EfiGraphicsOutputProtocolPixelInfo,
    size_of_info: u64,
    frame_buffer_base: usize,
    frame_buffer_size: usize,
}

#[repr(C)]
#[derive(Debug)]
struct EfiGraphicsOutputProtocol<'a> {
    _reserved: [u64; 3],
    mode: &'a EfiGraphicsOutputProtocolMode<'a>,
}

// GOP を SystemTable → BootServices → locate_protocol で取得
fn locate_graphic_protocol<'a>(
    efi_system_table: &EfiSystemTable,
) -> Result<&'a EfiGraphicsOutputProtocol<'a>> {
    let mut gop_ptr = null_mut::<EfiGraphicsOutputProtocol>();
    let status = (efi_system_table.boot_services.locate_protocol)(
        &EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID,
        null_mut::<EfiVoid>(),
        &mut gop_ptr as *mut *mut EfiGraphicsOutputProtocol as *mut *mut EfiVoid,
    );
    if status != EfiStatus::Success {
        return Err("Failed to locate graphics output protocol");
    }
    Ok(unsafe { &*gop_ptr })
}

// ===== 2D 画像抽象: Bitmap トレイト =====

trait Bitmap {
    fn bytes_per_pixel(&self) -> i64;
    fn pixels_per_line(&self) -> i64; // stride(1行に何ピクセル分並ぶか)
    fn width(&self) -> i64;
    fn height(&self) -> i64;
    fn buf_mut(&mut self) -> *mut u8; // 先頭ポインタ

    /// 範囲チェックなし（内部用）。(x,y) が有効であることが前提。
    unsafe fn unchecked_pixel_at_mut(&mut self, x: i64, y: i64) -> *mut u32 {
        self.buf_mut()
            .add(((y * self.pixels_per_line() + x) * self.bytes_per_pixel()) as usize)
            as *mut u32
    }

    /// 範囲チェックあり。安全に &mut u32 を返す。
    fn pixel_at_mut(&mut self, x: i64, y: i64) -> Option<&mut u32> {
        if self.is_in_x_range(x) && self.is_in_y_range(y) {
            // SAFETY: 直前で範囲チェック済み
            unsafe { Some(&mut *self.unchecked_pixel_at_mut(x, y)) }
        } else {
            None
        }
    }

    fn is_in_x_range(&self, px: i64) -> bool {
        0 <= px && px < core::cmp::min(self.width(), self.pixels_per_line())
    }
    fn is_in_y_range(&self, py: i64) -> bool {
        0 <= py && py < self.height()
    }
}

// ===== VRAM の実体 =====

#[derive(Clone, Copy)]
struct VramBufferInfo {
    buf: *mut u8,
    width: i64,
    height: i64,
    pixels_per_line: i64,
}

impl Bitmap for VramBufferInfo {
    #[inline]
    fn bytes_per_pixel(&self) -> i64 {
        4
    } // 4B/px 前提
    #[inline]
    fn pixels_per_line(&self) -> i64 {
        self.pixels_per_line
    }
    #[inline]
    fn width(&self) -> i64 {
        self.width
    }
    #[inline]
    fn height(&self) -> i64 {
        self.height
    }
    #[inline]
    fn buf_mut(&mut self) -> *mut u8 {
        self.buf
    }
}

// GOP 情報から VRAM ビューを作成
fn init_vram(efi_system_table: &EfiSystemTable) -> Result<VramBufferInfo> {
    let gp = locate_graphic_protocol(efi_system_table)?;
    Ok(VramBufferInfo {
        buf: gp.mode.frame_buffer_base as *mut u8,
        width: gp.mode.info.horizontal_resolution as i64,
        height: gp.mode.info.vertical_resolution as i64,
        pixels_per_line: gp.mode.info.pixels_per_scan_line as i64,
    })
}

// ===== 四角形と点, 直線を描く関数 =====

unsafe fn unchecked_draw_point<T: Bitmap>(buf: &mut T, color: u32, x: i64, y: i64) {
    *buf.unchecked_pixel_at_mut(x, y) = color;
}

fn draw_point<T: Bitmap>(buf: &mut T, color: u32, x: i64, y: i64) -> Result<()> {
    *(buf.pixel_at_mut(x, y).ok_or("Out of Range")?) = color;
    Ok(())
}

fn fill_rect<T: Bitmap>(buf: &mut T, color: u32, px: i64, py: i64, w: i64, h: i64) -> Result<()> {
    if !buf.is_in_x_range(px)
        || !buf.is_in_y_range(py)
        || !buf.is_in_x_range(px + w - 1)
        || !buf.is_in_y_range(py + h - 1)
    {
        return Err("Out of Range");
    }
    for y in py..py + h {
        for x in px..px + w {
            unsafe {
                unchecked_draw_point(buf, color, x, y);
            }
        }
    }
    Ok(())
}

fn calc_slope_point(da: i64, db: i64, ia: i64) -> Option<i64> {
    if da < db {
        None
    } else if da == 0 {
        Some(0)
    } else if (0..=da).contains(&ia) {
        Some((2 * db * ia + da) / da / 2)
    } else {
        None
    }
}

fn draw_line<T: Bitmap>(buf: &mut T, color: u32, x0: i64, y0: i64, x1: i64, y1: i64) -> Result<()> {
    if !buf.is_in_x_range(x0)
        || !buf.is_in_x_range(x1)
        || !buf.is_in_y_range(y0)
        || !buf.is_in_y_range(y1)
    {
        return Err("Out of Range");
    }

    let dx = (x1 - x0).abs(); // x差の大きさ
    let sx = (x1 - x0).signum(); // x方向(-1/0/+1)
    let dy = (y1 - y0).abs(); // y差の大きさ
    let sy = (y1 - y0).signum(); // y方向

    if dx >= dy {
        for (rx, ry) in (0..dx).flat_map(|rx| calc_slope_point(dx, dy, rx).map(|ry| (rx, ry))) {
            draw_point(buf, color, x0 + rx * sx, y0 + ry * sy)?;
        }
    } else {
        for (rx, ry) in (0..dy).flat_map(|ry| calc_slope_point(dy, dx, ry).map(|rx| (rx, ry))) {
            draw_point(buf, color, x0 + rx * sx, y0 + ry * sy)?;
        }
    }
    Ok(())
}

// ===== エントリポイント =====

#[no_mangle]
fn efi_main(_image_handle: EfiHandle, efi_system_table: &EfiSystemTable) {
    // 1) VRAM 初期化（UEFI → GOP → framebuffer/size/stride）
    let mut vram = init_vram(efi_system_table).expect("init_vram failed");

    let vw = vram.width;
    let vh = vram.height;
    fill_rect(&mut vram, 0x000000, 0, 0, vw, vh).expect("fill_rect failed");
    fill_rect(&mut vram, 0xff0000, 32, 32, 32, 32).expect("fill_rect failed");
    fill_rect(&mut vram, 0x00ff00, 64, 64, 64, 64).expect("fill_rect failed");
    fill_rect(&mut vram, 0x0000ff, 128, 128, 128, 128).expect("fill_rect failed");
    for i in 0..256 {
        let _ = draw_point(&mut vram, 0x010101 * i as u32, i, i);
    }

    let grid_size: i64 = 32;
    let rect_size: i64 = grid_size * 8;
    for i in (0..=rect_size).step_by(grid_size as usize) {
        let _ = draw_line(&mut vram, 0xffff00, 0, i, rect_size, i); // 横線
        let _ = draw_line(&mut vram, 0xffff00, i, 0, i, rect_size); // 縦線
    }
    let cx = rect_size / 2;
    let cy = rect_size / 2;
    for i in (0..=rect_size).step_by(grid_size as usize) {
        let _ = draw_line(&mut vram, 0xffff00, cx, cy, 0, i);
        let _ = draw_line(&mut vram, 0xffff00, cx, cy, i, 0);
        let _ = draw_line(&mut vram, 0xffff00, cx, cy, rect_size, i);
        let _ = draw_line(&mut vram, 0xffff00, cx, cy, i, rect_size);
    }

    hlt_loop();
}

// ===== パニックハンドラ =====

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    hlt_loop()
}
