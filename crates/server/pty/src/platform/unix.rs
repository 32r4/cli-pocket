pub(crate) fn default_shell() -> Vec<String> {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .map(|shell| vec![shell])
        .unwrap_or_else(|| vec!["/bin/sh".to_string()])
}
