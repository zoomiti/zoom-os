pub mod r#async;
pub mod once;

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
