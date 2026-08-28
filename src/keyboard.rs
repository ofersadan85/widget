use winit::keyboard::KeyCode;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KeyModifier(u8);
pub const CTRL: KeyModifier = KeyModifier(0b001);
pub const SHIFT: KeyModifier = KeyModifier(0b010);
pub const ALT: KeyModifier = KeyModifier(0b100);

impl KeyModifier {
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl std::ops::BitOr for KeyModifier {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for KeyModifier {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::BitXor for KeyModifier {
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Self(self.0 ^ rhs.0)
    }
}

impl std::ops::Not for KeyModifier {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

impl TryFrom<KeyCode> for KeyModifier {
    type Error = &'static str;

    fn try_from(value: KeyCode) -> Result<Self, Self::Error> {
        match value {
            KeyCode::ControlLeft | KeyCode::ControlRight => Ok(CTRL),
            KeyCode::ShiftLeft | KeyCode::ShiftRight => Ok(SHIFT),
            KeyCode::AltLeft | KeyCode::AltRight => Ok(ALT),
            _ => Err("Unsupported key code for KeyModifier"),
        }
    }
}

impl KeyModifier {
    pub fn press(&mut self, other: Self) {
        *self = *self | other;
    }

    pub fn release(&mut self, other: Self) {
        *self = *self & !other;
    }

    pub const fn as_step_seconds(self) -> f64 {
        if self.contains(CTRL) {
            60.0
        } else if self.contains(SHIFT) {
            10.0
        } else if self.contains(ALT) {
            1.0
        } else {
            3.0
        }
    }
}

/// Represents the audio volume level, ranging from 0.0 (muted) to 1.0 (maximum).
#[derive(Debug, Clone, Copy)]
pub struct Volume(f32);

impl Volume {
    pub fn adjust(&mut self, key: KeyCode) -> Result<(), &'static str> {
        match key {
            KeyCode::AudioVolumeUp => self.0 = (self.0 + 0.01).min(1.0),
            KeyCode::AudioVolumeDown => self.0 = (self.0 - 0.01).max(0.0),
            KeyCode::Numpad0 | KeyCode::AudioVolumeMute => self.0 = 0.0,
            KeyCode::Numpad1 => self.0 = 0.1,
            KeyCode::Numpad2 => self.0 = 0.2,
            KeyCode::Numpad3 => self.0 = 0.3,
            KeyCode::Numpad4 => self.0 = 0.4,
            KeyCode::Numpad5 => self.0 = 0.5,
            KeyCode::Numpad6 => self.0 = 0.6,
            KeyCode::Numpad7 => self.0 = 0.7,
            KeyCode::Numpad8 => self.0 = 0.8,
            KeyCode::Numpad9 => self.0 = 0.9,
            KeyCode::NumpadMultiply => self.0 = 1.0,
            _ => return Err("Unsupported key code for Volume"),
        }
        Ok(())
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_modifier_bitor() {
        assert_eq!((CTRL | SHIFT).0, 3);
        assert_eq!((CTRL | ALT).0, 5);
        assert_eq!((SHIFT | ALT).0, 6);
        assert_eq!((CTRL | SHIFT | ALT).0, 7);
    }

    #[test]
    fn key_modifier_bitand() {
        assert_eq!((CTRL & SHIFT).0, 0);
        assert_eq!((CTRL & CTRL).0, 1);
        assert_eq!((SHIFT & SHIFT).0, 2);
        assert_eq!((ALT & ALT).0, 4);
    }

    #[test]
    fn key_modifier_xor() {
        let base = CTRL | SHIFT | ALT;
        assert_eq!((base ^ CTRL ^ SHIFT).0, ALT.0);
        assert_eq!((base ^ CTRL ^ ALT).0, SHIFT.0);
        assert_eq!((base ^ SHIFT ^ ALT).0, CTRL.0);
    }
}
