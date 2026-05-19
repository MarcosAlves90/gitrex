#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Tui,
    Cli,
}

pub fn runtime_mode(is_interactive: bool) -> RuntimeMode {
    if is_interactive {
        RuntimeMode::Tui
    } else {
        RuntimeMode::Cli
    }
}

#[cfg(test)]
mod tests {
    use super::{runtime_mode, RuntimeMode};

    #[test]
    fn selects_tui_when_interactive() {
        assert_eq!(runtime_mode(true), RuntimeMode::Tui);
    }

    #[test]
    fn selects_cli_when_not_interactive() {
        assert_eq!(runtime_mode(false), RuntimeMode::Cli);
    }
}
