//! winit events → platform events → [`DeviceSample`]. No kernel types.

use klotho_core::YawMd;
use klotho_input::{Button, DeviceSample};
use winit::event::{DeviceEvent, ElementState, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::look::LookAccum;

/// Device-agnostic event the runtime can pump without depending on winit types
/// in tests.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum PlatEvent {
    /// Application close.
    Close,
    /// Framebuffer resize.
    Resize {
        /// Width, pixels.
        w: u32,
        /// Height, pixels.
        h: u32,
    },
    /// Digital button.
    Key {
        /// Bind-table button.
        button: Button,
        /// Pressed.
        down: bool,
    },
    /// Mouse-look millidegree delta.
    MouseDelta {
        /// Yaw delta.
        yaw_md: i32,
        /// Pitch delta (window space +down).
        pitch_md: i32,
    },
}

/// Map a winit window event. Keyboard / mouse buttons / resize / close.
#[must_use]
pub fn from_window_event(ev: &WindowEvent) -> Option<PlatEvent> {
    match ev {
        WindowEvent::CloseRequested => Some(PlatEvent::Close),
        WindowEvent::Resized(s) => Some(PlatEvent::Resize {
            w: s.width,
            h: s.height,
        }),
        WindowEvent::MouseInput { state, button, .. } => {
            if *button != MouseButton::Left {
                return None;
            }
            Some(PlatEvent::Key {
                button: Button::MouseLeft,
                down: *state == ElementState::Pressed,
            })
        }
        WindowEvent::KeyboardInput { event, .. } => {
            let PhysicalKey::Code(code) = event.physical_key else {
                return None;
            };
            let button = key_code(code)?;
            Some(PlatEvent::Key {
                button,
                down: event.state == ElementState::Pressed,
            })
        }
        _ => None,
    }
}

/// Map a winit device event (mouse motion → look millidegrees).
#[must_use]
pub fn from_device_event(ev: &DeviceEvent) -> Option<PlatEvent> {
    match ev {
        DeviceEvent::MouseMotion { delta: (dx, dy) } => {
            // ~0.05° per pixel. Presenters may float; this is still millidegrees.
            let yaw_md = (dx * 50.0) as i32;
            let pitch_md = (-dy * 50.0) as i32;
            Some(PlatEvent::MouseDelta { yaw_md, pitch_md })
        }
        _ => None,
    }
}

fn key_code(code: KeyCode) -> Option<Button> {
    Some(match code {
        KeyCode::KeyE => Button::KeyE,
        KeyCode::KeyF => Button::KeyF,
        KeyCode::KeyG => Button::KeyG,
        KeyCode::KeyT => Button::KeyT,
        KeyCode::KeyP => Button::KeyP,
        KeyCode::KeyR => Button::KeyR,
        KeyCode::Space => Button::KeySpace,
        KeyCode::Enter => Button::KeyEnter,
        _ => return None,
    })
}

/// Apply an event to the injected-device sample and look accum.
pub fn apply_event(sample: &mut DeviceSample, look: &mut LookAccum, ev: PlatEvent) {
    match ev {
        PlatEvent::Key { button, down } => {
            if down {
                sample.buttons.insert(button);
            } else {
                sample.buttons.remove(&button);
            }
        }
        PlatEvent::MouseDelta { yaw_md, pitch_md } => {
            look.apply_delta(yaw_md, pitch_md);
            sample.look_yaw = YawMd(yaw_md);
            sample.look_pitch = pitch_md;
        }
        PlatEvent::Close | PlatEvent::Resize { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{PlayerId, Tick};
    use klotho_input::InputMapper;
    use klotho_ir::Verb;

    use super::*;

    #[test]
    fn key_e_pumps_use() {
        let mut sample = DeviceSample::new(PlayerId(0), Tick(1));
        let mut look = LookAccum::new();
        apply_event(
            &mut sample,
            &mut look,
            PlatEvent::Key {
                button: Button::KeyE,
                down: true,
            },
        );
        let pi = InputMapper::hearth().map(&sample);
        assert_eq!(pi.verb, Verb::Use);
    }

    #[test]
    fn mouse_delta_updates_look() {
        let mut sample = DeviceSample::new(PlayerId(0), Tick(0));
        let mut look = LookAccum::new();
        apply_event(
            &mut sample,
            &mut look,
            PlatEvent::MouseDelta {
                yaw_md: 1_500,
                pitch_md: -500,
            },
        );
        assert_eq!(look.yaw, YawMd(1_500));
        assert_eq!(look.pitch_md, -500);
        assert_eq!(sample.look_yaw, YawMd(1_500));
    }
}
