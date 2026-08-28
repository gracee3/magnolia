#[cfg(target_arch = "wasm32")]
mod client;
#[cfg(target_arch = "wasm32")]
mod dense;
#[cfg(target_arch = "wasm32")]
mod model;
#[cfg(target_arch = "wasm32")]
mod studio;
#[cfg(target_arch = "wasm32")]
mod workspace;

#[cfg(target_arch = "wasm32")]
fn main() {
    leptos::mount::mount_to_body(studio::App);
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("magnolia-studio-web is a WebAssembly CSR application; use Trunk to build it");
}
