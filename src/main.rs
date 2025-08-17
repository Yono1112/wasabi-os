// src/main.rs
#![no_std]
#![no_main]
#![feature(offset_of)]

use core::arch::asm;
use core::fmt::Write;
use core::panic::PanicInfo;
use core::writeln;
use wasabi_os::graphics::draw_test_pattern;
use wasabi_os::graphics::fill_rect;
use wasabi_os::graphics::Bitmap;
use wasabi_os::uefi::exit_from_efi_boot_services;
use wasabi_os::uefi::init_vram;
use wasabi_os::uefi::EfiHandle;
use wasabi_os::uefi::EfiMemoryType;
use wasabi_os::uefi::EfiSystemTable;
use wasabi_os::uefi::MemoryMapHolder;
use wasabi_os::uefi::VramTextWriter;

#[inline(always)]
fn hlt_loop() -> ! {
    loop {
        unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) }
    }
}

// ===== エントリポイント =====

#[no_mangle]
fn efi_main(image_handle: EfiHandle, efi_system_table: &EfiSystemTable) {
    // VRAM 初期化（UEFI → GOP → framebuffer/size/stride）
    let mut vram = init_vram(efi_system_table).expect("init_vram failed");
    let vw = vram.width();
    let vh = vram.height();

    fill_rect(&mut vram, 0x000000, 0, 0, vw, vh).expect("fill_rect failed");

    draw_test_pattern(&mut vram);

    let mut w = VramTextWriter::new(&mut vram);
    // for i in 0..4 {
    //     writeln!(w, "i = {i}").unwrap();
    // }
    writeln!(w, "vw: {vw}, vh: {vh}").unwrap();

    // メモリマップの表示
    let mut memory_map = MemoryMapHolder::new();
    let status = efi_system_table
        .boot_services()
        .get_memory_map(&mut memory_map);
    writeln!(w, "{status:?}").unwrap();
    let mut total_memory_pages = 0;
    for e in memory_map.iter() {
        if e.memory_type() != EfiMemoryType::CONVENTIONAL_MEMORY {
            continue;
        }
        total_memory_pages += e.number_of_pages();
        // writeln!(w, "{e:?}").unwrap();
    }
    let total_memory_size_mib = total_memory_pages * 4096 / 1024 / 1024;
    writeln!(
        w,
        "Total: {total_memory_pages} pages = {total_memory_size_mib} MiB"
    )
    .unwrap();

    exit_from_efi_boot_services(image_handle, efi_system_table, &mut memory_map);
    writeln!(w, "Hello Non-UEFI world!").unwrap();

    hlt_loop();
}

// ===== パニックハンドラ =====

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    hlt_loop()
}
