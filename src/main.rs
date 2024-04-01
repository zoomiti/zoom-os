use clap::{Parser, ValueEnum};
use std::{
    env,
    process::{self, Command},
};

/// QEMU runner for zoom_os
#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[arg(short, long, value_enum, default_value = "uefi")]
    boot: BootType,
}

#[derive(Clone, Copy, ValueEnum, Default)]
enum BootType {
    Bios,
    #[default]
    Uefi,
}

fn main() {
    let args = Args::parse();
    let mut qemu = Command::new("qemu-system-x86_64");
    qemu.arg("-drive");

    match args.boot {
        BootType::Uefi => {
            println!("UEFI path {}", env!("UEFI_IMAGE"));
            qemu.arg(format!("format=raw,file={}", env!("UEFI_IMAGE")));
            qemu.arg("-bios").arg(ovmf_prebuilt::ovmf_pure_efi());
        }
        BootType::Bios => {
            println!("BIOS path {}", env!("BIOS_IMAGE"));
            qemu.arg(format!("format=raw,file={}", env!("BIOS_IMAGE")));
        }
    }
    qemu.arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    qemu.arg("-serial").arg("stdio");
    let exit_status = qemu.status().unwrap();
    process::exit(exit_status.code().unwrap_or(-1));
}
