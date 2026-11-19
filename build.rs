fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "MWCleaner");
        res.set("FileDescription", "Windows 清理与优化工具");
        res.set("ProductVersion", "0.4.0");
        res.set("FileVersion", "0.4.0");
        res.compile().expect("嵌入 Windows 资源失败");
    }
}