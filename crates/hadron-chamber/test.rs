fn main() {
    let p = std::env::current_dir().unwrap().join("..").join("..").join("assets").join("hadron.ico");
    println!("Exists: {}", p.exists());
}
