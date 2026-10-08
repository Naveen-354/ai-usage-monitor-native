#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragEvent {
    None,
    StartDrag,
    Click,
}

#[derive(Default)]
pub struct DragOrClick {
    origin: Option<(f32, f32)>,
    drag_started: bool,
}

const DRAG_THRESHOLD_PX: f32 = 4.0;

impl DragOrClick {
    pub fn new() -> Self {
        Self {
            origin: None,
            drag_started: false,
        }
    }

    pub fn on_press(&mut self, x: f32, y: f32, is_primary_button: bool) {
        if is_primary_button {
            self.origin = Some((x, y));
            self.drag_started = false;
        }
    }

    pub fn on_move(&mut self, x: f32, y: f32) -> DragEvent {
        if let Some((ox, oy)) = self.origin {
            if !self.drag_started {
                let dx = x - ox;
                let dy = y - oy;
                if (dx * dx + dy * dy).sqrt() > DRAG_THRESHOLD_PX {
                    self.drag_started = true;
                    return DragEvent::StartDrag;
                }
            }
        }
        DragEvent::None
    }

    pub fn on_release(&mut self) -> DragEvent {
        let mut evt = DragEvent::None;
        if self.origin.is_some() {
            if !self.drag_started {
                evt = DragEvent::Click;
            }
            self.origin = None;
            self.drag_started = false;
        }
        evt
    }

    pub fn on_cancel(&mut self) {
        self.origin = None;
        self.drag_started = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drag_or_click() {
        let mut d = DragOrClick::new();
        d.on_press(0.0, 0.0, true);
        assert_eq!(d.on_move(1.0, 1.0), DragEvent::None);
        assert_eq!(d.on_release(), DragEvent::Click);

        d.on_press(0.0, 0.0, true);
        assert_eq!(d.on_move(5.0, 0.0), DragEvent::StartDrag);
        assert_eq!(d.on_move(10.0, 0.0), DragEvent::None);
        assert_eq!(d.on_release(), DragEvent::None);

        d.on_press(0.0, 0.0, false);
        assert_eq!(d.on_release(), DragEvent::None);
    }
}
