use std::env;
use std::path::PathBuf;

fn find_parakeet_cpp_build_dir() -> Option<PathBuf> {
    // Allow explicit override.
    if let Ok(p) = env::var("PARAKEET_CPP_BUILD_DIR") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }

    // Common dev layout in this workspace:
    //   /home/emmy/git/magnolia/apps/asr_test
    //   /home/emmy/git/parakeet/cpp/build
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    let git_root = manifest_dir.parent()?.parent()?.parent()?; // .../git
    Some(git_root.join("parakeet").join("cpp").join("build"))
}

fn main() {
    if let Some(build_dir) = find_parakeet_cpp_build_dir() {
        if build_dir.exists() {
            // Link-time search path (helps the final link step).
            println!("cargo:rustc-link-search=native={}", build_dir.display());
            // Runtime search path so `cargo run` works without LD_LIBRARY_PATH.
            println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", build_dir.display());
        } else {
            println!(
                "cargo:warning=Parakeet C++ build dir not found at {} (set PARAKEET_CPP_BUILD_DIR or build Parakeet: cmake -S parakeet/cpp -B parakeet/cpp/build && cmake --build parakeet/cpp/build -j)",
                build_dir.display()
            );
        }
    }

    println!("cargo:rerun-if-env-changed=PARAKEET_CPP_BUILD_DIR");
}



