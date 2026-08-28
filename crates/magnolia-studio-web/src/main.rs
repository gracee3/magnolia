#[cfg(target_arch = "wasm32")]
mod client;

#[cfg(target_arch = "wasm32")]
fn main() {
    use leptos::prelude::*;

    leptos::mount::mount_to_body(|| {
        view! {
            <main aria-label="Magnolia transport proof">
                <h1>"Magnolia"</h1>
                <p>"Native/browser transport spine"</p>
            </main>
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("magnolia-studio-web is a WebAssembly CSR application; use Trunk to build it");
}
