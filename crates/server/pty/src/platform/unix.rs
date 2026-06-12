pub(crate) fn default_shell() -> Vec<String> {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .map_or_else(|| vec!["/bin/sh".to_string()], |shell| vec![shell])
}
