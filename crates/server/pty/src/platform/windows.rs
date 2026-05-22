pub(crate) fn default_shell() -> Vec<String> {
    if std::path::Path::new(r"C:\Windows\System32\cmd.exe").exists() {
        vec![r"C:\Windows\System32\cmd.exe".to_string()]
    } else {
        vec!["powershell.exe".to_string()]
    }
}
