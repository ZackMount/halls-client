//! Organizer-only cross-language battery reference.
use halls::battery::Battery;
use std::io::Write;
fn main() -> std::io::Result<()> {
    let mut out = std::fs::File::create(std::env::args().nth(1).unwrap())?;
    let mut battery = Battery::default();
    let mut on = true;
    for i in 0..12001 {
        let dt: f32 = if i == 0 {
            0.0
        } else {
            [0.05, 0.02, 0.016, 0.033][i % 4]
        };
        let toggle = i > 6500 && i % 31 == 0;
        if toggle {
            battery.toggle();
        }
        battery.tick(&mut on, dt);
        for value in [
            dt.to_bits(),
            toggle as u32,
            on as u32,
            battery.charge.to_bits(),
            (battery.age as f32).to_bits(),
            battery.wanted as u32,
        ] {
            out.write_all(&value.to_le_bytes())?;
        }
    }
    Ok(())
}
