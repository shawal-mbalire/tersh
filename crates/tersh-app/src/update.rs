//! The update use case: find out whether a newer release exists and install it.

use anyhow::Result;
use tersh_domain::update::is_newer;

use crate::ports::{Installer, ReleaseSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate { current: String },
    Available { current: String, latest: String },
    Installed { previous: String, installed: String },
}

/// Whether a newer release than `current` exists.
pub fn check(current: &str, releases: &dyn ReleaseSource) -> Result<UpdateStatus> {
    let latest = releases.latest()?;
    Ok(if is_newer(&latest, current) {
        UpdateStatus::Available { current: current.into(), latest }
    } else {
        UpdateStatus::UpToDate { current: current.into() }
    })
}

/// Install the latest release if it is newer than `current`.
pub fn update(current: &str, releases: &dyn ReleaseSource, installer: &dyn Installer) -> Result<UpdateStatus> {
    match check(current, releases)? {
        UpdateStatus::Available { current, latest } => {
            installer.install(&latest)?;
            Ok(UpdateStatus::Installed { previous: current, installed: latest })
        },
        status => Ok(status),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct Latest(&'static str);
    impl ReleaseSource for Latest {
        fn latest(&self) -> Result<String> {
            Ok(self.0.into())
        }
    }

    #[derive(Default)]
    struct Recorder(RefCell<Vec<String>>);
    impl Installer for Recorder {
        fn install(&self, tag: &str) -> Result<()> {
            self.0.borrow_mut().push(tag.into());
            Ok(())
        }
    }

    #[test]
    fn installs_only_newer_releases() {
        let installer = Recorder::default();
        assert_eq!(update("0.2.0", &Latest("v0.2.0"), &installer).unwrap(), UpdateStatus::UpToDate { current: "0.2.0".into() });
        assert!(installer.0.borrow().is_empty());

        let status = update("0.2.0", &Latest("v0.3.0"), &installer).unwrap();
        assert_eq!(status, UpdateStatus::Installed { previous: "0.2.0".into(), installed: "v0.3.0".into() });
        assert_eq!(*installer.0.borrow(), ["v0.3.0"]);
    }

    #[test]
    fn check_does_not_install() {
        assert_eq!(
            check("0.1.0", &Latest("v0.2.0")).unwrap(),
            UpdateStatus::Available { current: "0.1.0".into(), latest: "v0.2.0".into() }
        );
    }
}
