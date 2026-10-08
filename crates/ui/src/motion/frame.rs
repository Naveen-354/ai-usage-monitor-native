pub fn next_repaint(fire_burning: bool, counter_animating: bool) -> Option<std::time::Duration> {
    if counter_animating {
        Some(std::time::Duration::from_millis(33)) // ~30 fps
    } else if fire_burning {
        Some(std::time::Duration::from_millis(83)) // ~12 fps
    } else {
        None // idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_repaint() {
        assert_eq!(next_repaint(false, false), None);
        assert_eq!(next_repaint(true, false), Some(std::time::Duration::from_millis(83)));
        assert_eq!(next_repaint(false, true), Some(std::time::Duration::from_millis(33)));
        assert_eq!(next_repaint(true, true), Some(std::time::Duration::from_millis(33))); // counter overrides fire
    }
}
