use std::env;
use std::path::Path;

pub struct EnvGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvGuard {
    pub fn new(key: &'static str, value: &str) -> Self {
        let old = env::var(key).ok();
        unsafe {
            env::set_var(key, value);
        }
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(v) => unsafe { env::set_var(self.key, v) },
            None => unsafe { env::remove_var(self.key) },
        }
    }
}

pub fn run_with_cwd<F, R>(dir: &Path, f: F) -> R
where
    F: FnOnce() -> R,
{
    let original = env::current_dir().expect("failed to get cwd");

    env::set_current_dir(dir).expect("failed to set cwd");

    let result = f();

    env::set_current_dir(original).expect("failed to restore cwd");

    result
}
