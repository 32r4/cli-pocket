#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
#[allow(dead_code)]
pub(crate) fn default_shell() -> Vec<String> {
    unix::default_shell()
}

#[cfg(windows)]
#[allow(dead_code)]
pub(crate) fn default_shell() -> Vec<String> {
    windows::default_shell()
}
