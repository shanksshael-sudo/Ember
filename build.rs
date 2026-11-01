use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/config.rs.in");

    let config_path = Path::new("src/config.rs");
    if !config_path.exists() {
        let default_config = r#"// Auto-generated fallback configuration for Ember
pub static VERSION: &str = "2026.7.0";
pub static GETTEXT_PACKAGE: &str = "amberol";
pub static LOCALEDIR: &str = "/usr/share/locale";
pub static PKGDATADIR: &str = "/usr/share/amberol";
pub static APPLICATION_ID: &str = "io.bassi.Amberol";
pub static PROFILE: &str = "";
"#;
        let _ = fs::write(config_path, default_config);
    }
}
