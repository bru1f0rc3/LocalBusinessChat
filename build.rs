fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    win7_import_libs();
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app.ico");
        res.set("FileDescription", "Nassi RS — LAN messenger");
        res.set("ProductName", "Nassi RS");
        // Common Controls v6 (tray / menu subclassing on Windows 7) and the supported
        // Windows versions: 7, 8, 8.1, 10/11.
        res.set_manifest(MANIFEST);
        if let Err(e) = res.compile() {
            println!("cargo:warning=icon resource not embedded: {e}");
        }
    }
}

/// Windows 7/8.1 builds (*-win7-windows-msvc): windows-targets 0.48 only adds its import
/// library for the usual target names, so point the linker at it ourselves. The crate is a
/// direct dependency (Cargo.toml), so it is always unpacked in the cargo registry.
fn win7_import_libs() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.ends_with("-win7-windows-msvc") {
        return;
    }
    let krate = if target.starts_with("x86_64") { "windows_x86_64_msvc-0.48.5" } else { "windows_i686_msvc-0.48.5" };
    let home = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join(".cargo")));
    let Some(home) = home else { return };
    let Ok(rd) = std::fs::read_dir(home.join("registry").join("src")) else { return };
    for e in rd.flatten() {
        let lib = e.path().join(krate).join("lib");
        if lib.join("windows.0.48.5.lib").exists() {
            println!("cargo:rustc-link-search=native={}", lib.display());
            return;
        }
    }
    println!("cargo:warning={krate} not found in the cargo registry: linking may fail");
}

#[cfg(windows)]
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="NassiRS" version="1.0.0.0"/>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
    </windowsSettings>
  </application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;
