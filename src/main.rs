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
fn locate_graphic_protocol<'a>(st: &EfiSystemTable) -> Result<&'a EfiGraphicsOutputProtocol<'a>> {
    let mut gop_ptr = null_mut::<EfiGraphicsOutputProtocol>();
    let status = (st.boot_services.locate_protocol)(
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
fn init_vram(st: &EfiSystemTable) -> Result<VramBufferInfo> {
    let gp = locate_graphic_protocol(st)?;
    Ok(VramBufferInfo {
        buf: gp.mode.frame_buffer_base as *mut u8,
        width: gp.mode.info.horizontal_resolution as i64,
        height: gp.mode.info.vertical_resolution as i64,
        pixels_per_line: gp.mode.info.pixels_per_scan_line as i64,
    })
}

// ===== エントリポイント =====

#[no_mangle]
fn efi_main(_image_handle: EfiHandle, st: &EfiSystemTable) {
    // 1) VRAM 初期化（UEFI → GOP → framebuffer/size/stride）
    let mut vram = match init_vram(st) {
        Ok(v) => v,
        Err(_) => hlt_loop(),
    };

    // 2) 全画面塗りつぶし（例：緑 0x00ff00）
    for y in 0..vram.height() {
        for x in 0..vram.width() {
            if let Some(px) = vram.pixel_at_mut(x, y) {
                *px = 0x00ff00; // ※BGR 環境では色が入れ替わることがあります
            }
        }
    }

    // 3) 停止
    hlt_loop();
}

// ===== パニックハンドラ =====

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    hlt_loop()
}
