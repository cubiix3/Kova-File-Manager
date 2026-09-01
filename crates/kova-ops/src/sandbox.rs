use std::path::{Path, PathBuf};

/// Root guard for integration tests that mutate the filesystem.
///
/// Every mutating operation checks that its target path is inside this root.
/// The guard is intentionally cheap to create and validates on first use.
#[derive(Debug, Clone)]
pub struct TestSandbox {
    root: PathBuf,
}

impl TestSandbox {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return true if `path` is inside the sandbox root.
    pub fn contains(&self, path: &Path) -> bool {
        let Ok(canonical_root) = std::fs::canonicalize(&self.root) else {
            return false;
        };
        let canonical_target = match path.is_absolute() {
            true => path.to_path_buf(),
            false => {
                let mut combined = canonical_root.clone();
                combined.push(path);
                combined
            }
        };

        let Ok(canonical_target) = std::fs::canonicalize(&canonical_target)
            .or_else(|_| Ok::<_, std::io::Error>(canonical_target.clone()))
        else {
            return false;
        };

        canonical_target.starts_with(&canonical_root)
    }

    /// Validate that `path` is inside the sandbox root. Returns the absolute
    /// path on success.
    pub fn require_inside(&self, path: &Path) -> Result<PathBuf, std::io::Error> {
        if self.contains(path) {
            Ok(path.to_path_buf())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("path {} is outside the test sandbox", path.display()),
            ))
        }
    }

    /// Create a unique subdirectory under the sandbox root.
    pub fn create_unique_dir(&self) -> Result<PathBuf, std::io::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        let dir = self.root.join(format!("kova-test-{id}"));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

/// Create the standard temporary sandbox root used by integration tests.
#[cfg(windows)]
pub fn temp_sandbox() -> Result<TestSandbox, std::io::Error> {
    let temp = std::env::temp_dir();
    let root = temp.join("kova-tests");
    std::fs::create_dir_all(&root)?;
    Ok(TestSandbox::new(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sandbox_contains_children_and_rejects_outside() {
        let root = std::env::temp_dir().join(format!("kova-sandbox-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sandbox = TestSandbox::new(root.clone());
        let child = root.join("child");
        fs::create_dir_all(&child).unwrap();
        assert!(sandbox.contains(&child));
        assert!(!sandbox.contains(Path::new("C:\\Windows")));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn sandbox_rejects_outside_path_for_mutation() {
        let root = std::env::temp_dir().join(format!("kova-sandbox-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sandbox = TestSandbox::new(root.clone());

        let outside = Path::new("C:\\Windows\\notepad.exe");
        let result = sandbox.require_inside(outside);
        assert!(result.is_err(), "must reject outside path");
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );

        fs::remove_dir_all(&root).ok();
    }
}
