const WINDOWS_POWERSHELL_PATH: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
const CMD_PATH: &str = r"C:\Windows\System32\cmd.exe";

pub(crate) fn default_shell() -> Vec<String> {
    if std::path::Path::new(WINDOWS_POWERSHELL_PATH).exists() {
        vec![WINDOWS_POWERSHELL_PATH.to_string()]
    } else if std::path::Path::new(CMD_PATH).exists() {
        vec![CMD_PATH.to_string()]
    } else {
        vec!["powershell.exe".to_string()]
    }
}
