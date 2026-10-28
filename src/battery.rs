#[allow(unused_imports)]
use crate::prelude::*;

/// Simulation-time lamp state; render rate and pause duration never enter here.
pub struct Battery {
    pub charge: f32,
    pub age: f64,
    pub wanted: bool,
}
impl Default for Battery {
    fn default() -> Self {
        Self {
            charge: 100.0,
            age: 0.0,
            wanted: true,
        }
    }
}
impl Battery {
    pub fn toggle(&mut self) -> bool {
        if self.wanted || self.charge >= 1.0 {
            self.wanted = !self.wanted;
            true
        } else {
            false
        }
    }
    pub fn tick(&mut self, on: &mut bool, dt: f32) {
        // First account for the interval that was actually lit. A switch is
        // applied at this frame boundary, so switching off never waits.
        let mut spent = 0.0;
        if *on {
            spent = dt.min(self.charge.max(0.0));
            self.charge = (self.charge - spent).max(0.0);
            self.age += spent as f64;
            if self.charge == 0.0 {
                *on = false;
                self.wanted = false;
                self.age = 0.0;
            }
        }
        if !*on {
            let rest = (dt - spent) as f64;
            let charging = (rest - (3.0 - self.age).max(0.0)).max(0.0);
            self.charge = (self.charge + (charging * 4.0) as f32).min(100.0);
            self.age = (self.age + rest).min(3.0);
        }
        if *on && !self.wanted {
            // Settle this activation to whole seconds. Double precision plus
            // a 10-microsecond rounding guard avoids charging 2 s for 20 x .05 s.
            let billed = ((self.age - 0.00001) as f32).ceil().max(1.0);
            let extra = (billed as f64 - self.age).max(0.0) as f32;
            self.charge = (self.charge - extra).max(0.0);
            *on = false;
            self.age = 0.0;
        } else if !*on && self.wanted {
            if self.charge >= 1.0 {
                *on = true;
                self.age = 0.0;
            } else {
                self.wanted = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hundred_seconds_drain_and_delayed_twenty_five_seconds_charge() {
        let mut battery = Battery::default();
        let mut on = true;
        for _ in 0..2000 {
            battery.tick(&mut on, 0.05);
        }
        assert!(battery.charge < 0.005);
        battery.tick(&mut on, 0.05);
        assert!(!on && !battery.wanted);
        let mut battery = Battery {
            charge: 0.0,
            age: 0.0,
            wanted: false,
        };
        for _ in 0..59 {
            battery.tick(&mut on, 0.05);
        }
        assert_eq!(battery.charge, 0.0);
        for _ in 0..501 {
            battery.tick(&mut on, 0.05);
        }
        assert!((battery.charge - 100.0).abs() < 0.005);
        assert!(!on, "recharging must not restart the lamp automatically");
    }
    #[test]
    fn switching_off_is_immediate_but_each_activation_rounds_up() {
        for (duration, cost) in [
            (0.0, 1.0),
            (0.01, 1.0),
            (0.99, 1.0),
            (1.0, 1.0),
            (1.01, 2.0),
            (2.01, 3.0),
        ] {
            let mut battery = Battery::default();
            let mut on = true;
            battery.tick(&mut on, duration);
            battery.toggle();
            battery.tick(&mut on, 0.0);
            assert!(!on, "switch must not wait at {duration} s");
            assert!((battery.charge - (100.0 - cost)).abs() < 0.0001);
            assert_eq!(battery.age, 0.0);
            battery.tick(&mut on, 3.0);
            assert!((battery.charge - (100.0 - cost)).abs() < 0.0001);
            battery.tick(&mut on, 0.125);
            assert!((battery.charge - (100.5 - cost)).abs() < 0.0001);
        }
    }
    #[test]
    fn exact_second_frame_sums_do_not_bill_an_extra_second() {
        for (dt, count) in [(0.05, 20), (1.0 / 30.0, 30), (0.01, 100), (0.02, 3000)] {
            let mut battery = Battery::default();
            let mut on = true;
            for _ in 0..count {
                battery.tick(&mut on, dt);
            }
            battery.toggle();
            battery.tick(&mut on, 0.0);
            assert!((battery.charge - (100.0 - dt * count as f32)).abs() < 0.02);
        }
    }
    #[test]
    fn ignored_low_charge_presses_do_not_count_as_request_changes() {
        let mut battery = Battery {
            charge: 0.5,
            age: 1.0,
            wanted: true,
        };
        let mut changed = false;
        for _ in 0..2 {
            changed ^= battery.toggle();
        }
        assert!(changed);
        assert!(!battery.wanted);
    }
    #[test]
    fn repeated_flashes_do_not_bypass_drain_or_the_recharge_delay() {
        let mut battery = Battery::default();
        let mut on = true;
        for _ in 0..10 {
            battery.tick(&mut on, 0.01);
            battery.toggle();
            battery.tick(&mut on, 0.0);
            assert!(!on);
            battery.tick(&mut on, 2.0);
            battery.toggle();
            battery.tick(&mut on, 0.0);
            assert!(on);
        }
        assert_eq!(battery.charge, 90.0);
        battery.charge = 0.5;
        battery.wanted = false;
        battery.toggle();
        assert!(!battery.wanted, "a sub-second reserve cannot be strobed");
    }
}
