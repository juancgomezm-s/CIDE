pub const VERSION: &str = env!("CIDE_RELEASE_VERSION");
pub const REVISION: &str = env!("CIDE_REVISION");
pub const OWNER: &str = "Juan Carlos Gómez Montiel";

pub fn text() -> String {
    format!(
        "CIDE {}\nJuan Carlos Gómez Montiel\n\nRevisión {}\nSistema {} · {}",
        env!("CIDE_RELEASE_VERSION"),
        env!("CIDE_REVISION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}
