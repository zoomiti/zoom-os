use std::{
    env,
    process::{self, Command},
};

fn main() {
    let mut qemu = Command::new("qemu-system-x86_64");
    qemu.arg("-drive");

    let uefi = true;
    if uefi {
        qemu.arg(format!("format=raw,file={}", env!("UEFI_IMAGE")));
        qemu.arg("-bios").arg(ovmf_prebuilt::ovmf_pure_efi());
    } else {
        qemu.arg(format!("format=raw,file={}", env!("BIOS_IMAGE")));
    }
    qemu.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    qemu.arg("-serial").arg("stdio");
    let exit_status = qemu.status().unwrap();
    process::exit(exit_status.code().unwrap_or(-1));
}
