use auto_launch::{AutoLaunch, AutoLaunchBuilder};

/// The command-line arguments the start-up entry launches the app with: none. The overlay is meant to be on screen
/// from the first moment, so there is no "start minimised"; the app's own tests parse exactly these arguments.
pub const LAUNCH_ARGS: &[&str] = &[];

pub struct Autostart {
    app_name: String,
    app_path: String,
}

impl Autostart {
    pub fn new(app_name: &str, app_path: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            app_path: app_path.to_string(),
        }
    }

    #[allow(deprecated)]
    fn builder(&self) -> AutoLaunch {
        AutoLaunchBuilder::new()
            .set_app_name(&self.app_name)
            .set_app_path(&self.app_path)
            .set_use_launch_agent(false)
            .set_args(LAUNCH_ARGS)
            .build()
            .unwrap()
    }

    pub fn enable(&self) -> Result<(), Box<dyn std::error::Error>> {
        let al = self.builder();
        al.enable()?;
        Ok(())
    }

    pub fn disable(&self) -> Result<(), Box<dyn std::error::Error>> {
        let al = self.builder();
        al.disable()?;
        Ok(())
    }

    pub fn is_enabled(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let al = self.builder();
        Ok(al.is_enabled()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_up_entry_passes_no_arguments() {
        assert!(LAUNCH_ARGS.is_empty());
    }
}
