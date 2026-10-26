//! Recursos do Windows embutidos no executável.
//!
//! Sem isto o `dish.exe` não tem ícone próprio e o atalho do menu Iniciar (e a
//! barra de tarefas) mostram o ícone genérico do Windows.
//!
//! O bloco é `cfg(windows)` porque `winresource` só existe como dependência de
//! build no Windows: builds nativos em Linux/macOS não pagam por ela. Compilar
//! de Linux para Windows (cross) não embute os recursos — o artefato de
//! referência é o build nativo do CI.

#[cfg(windows)]
fn embed_resources() {
    println!("cargo:rerun-if-changed=assets/dish-icon.ico");
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/dish-icon.ico");
    resource.set("ProductName", "Dish");
    resource.set("CompanyName", "Cleison Santos");
    resource.set("FileVersion", env!("CARGO_PKG_VERSION"));
    resource.set("ProductVersion", env!("CARGO_PKG_VERSION"));
    resource.set("FileDescription", "Dish - desktop interface for the Pi coding agent");
    resource.set("LegalCopyright", "Apache-2.0");
    resource.set("OriginalFilename", "dish.exe");
    if let Err(error) = resource.compile() {
        panic!("não foi possível embutir os recursos do Windows: {error}");
    }
}

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        #[cfg(windows)]
        embed_resources();
    }
}
