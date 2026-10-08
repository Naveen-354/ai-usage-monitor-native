use global_hotkey::hotkey::{Code, HotKey, Modifiers};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Shortcut {
    pub modifiers: Modifiers,
    pub key: Code,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    UnknownKey(String),
}

pub fn parse_shortcut(s: &str) -> Result<Shortcut, ParseError> {
    if s.trim().is_empty() {
        return Err(ParseError::Empty);
    }
    
    let parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
    let mut mods = Modifiers::empty();
    let mut key = Code::KeyA; // default placeholder
    
    for (i, p) in parts.iter().enumerate() {
        let p_lower = p.to_lowercase();
        if i < parts.len() - 1 {
            match p_lower.as_str() {
                "ctrl" | "control" | "commandorcontrol" => mods.insert(Modifiers::CONTROL),
                "alt" => mods.insert(Modifiers::ALT),
                "shift" => mods.insert(Modifiers::SHIFT),
                "super" | "win" | "command" | "meta" => mods.insert(Modifiers::SUPER),
                _ => {}
            }
        } else {
            key = match p_lower.as_str() {
                "u" => Code::KeyU,
                "e" => Code::KeyE,
                "f" => Code::KeyF,
                "t" => Code::KeyT,
                "a" => Code::KeyA,
                "b" => Code::KeyB,
                "c" => Code::KeyC,
                "d" => Code::KeyD,
                "g" => Code::KeyG,
                "h" => Code::KeyH,
                "i" => Code::KeyI,
                "j" => Code::KeyJ,
                "k" => Code::KeyK,
                "l" => Code::KeyL,
                "m" => Code::KeyM,
                "n" => Code::KeyN,
                "o" => Code::KeyO,
                "p" => Code::KeyP,
                "q" => Code::KeyQ,
                "r" => Code::KeyR,
                "s" => Code::KeyS,
                "v" => Code::KeyV,
                "w" => Code::KeyW,
                "x" => Code::KeyX,
                "y" => Code::KeyY,
                "z" => Code::KeyZ,
                _ => return Err(ParseError::UnknownKey(p.to_string())),
            };
        }
    }
    
    Ok(Shortcut { modifiers: mods, key })
}

impl Shortcut {
    pub fn to_global(&self) -> HotKey {
        HotKey::new(Some(self.modifiers), self.key)
    }
}

pub enum HotkeyAction {
    Toggle,
    Expand,
    Focus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shortcut() {
        let sc = parse_shortcut("Ctrl+Alt+T").unwrap();
        assert_eq!(sc.modifiers, Modifiers::CONTROL | Modifiers::ALT);
        assert_eq!(sc.key, Code::KeyT);

        let sc2 = parse_shortcut("CommandOrControl+Shift+U").unwrap();
        assert_eq!(sc2.modifiers, Modifiers::CONTROL | Modifiers::SHIFT);
        assert_eq!(sc2.key, Code::KeyU);

        let sc3 = parse_shortcut("Alt+F").unwrap();
        assert_eq!(sc3.modifiers, Modifiers::ALT);
        assert_eq!(sc3.key, Code::KeyF);
        
        assert!(parse_shortcut("Ctrl+Unknown").is_err());
        assert!(parse_shortcut("").is_err());
    }
}
