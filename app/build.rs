fn main() {
    // O tauri_build só observa o tauri.conf.json e as capabilities; sem isto,
    // trocar o ícone ou a interface não chega ao executável.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=ui");
    tauri_build::build()
}
