fn main() {
    // Embed app icon and metadata into the Windows .exe resource table.
    #[cfg(target_os = "windows")]
    {
        let icon_path = std::path::Path::new("assets/icon.ico");
        if icon_path.exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icon.ico");
            res.set("ProductName", "Glass");
            res.set("FileDescription", "GPU-accelerated terminal emulator");
            res.compile().expect("Failed to compile Windows resources");
        } else {
            println!("cargo:warning=assets/icon.ico not found — building without embedded icon");
        }
    }
}
