fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../../assets/hadron.ico");
        if let Err(e) = res.compile() {
            eprintln!("winres error: {e}");
        }
    }
}
