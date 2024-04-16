# Zoom OS
This is a hobby OS written in Rust for the sake of learning. It started out by
following Philipp Oppermann's blog which is hosted
[here](https://os.phil-opp.com/)


## Existing and Planned Features

- [x] Booting from Bios and UEFI
- []  APIC initialization (works in QEMU but not on real hardware in my testing)
- [x] RTC communication (used for system and monotonic time with hardware interrupts)
- [x] Physical Frame Allocation in 2 stages
- [x] Heap Allocation
- [x] PS/2 Keyboard support
- [x] Double Buffered frame buffer support
- [x] Task support using Rust's Async/Await model
- [x] Basic Synchronization and Lazy initialization primitives
- [x] Tracing support
- [] Basic USB support
- [] Basic PCI support
- [] Multicore (SMP) support
- [] Unit and Integration testing (broke when transitioning to UEFI boot)
