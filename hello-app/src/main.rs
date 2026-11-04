use std::env;
use std::path::Path;

fn main() {
    println!("Hello from an app");
    match env::args_os().nth(1) {
        Some(arg) => {
            let path = Path::new(&arg);
            let name = path.file_name().unwrap_or(path.as_os_str());
            println!("Targeted file: {}", name.to_string_lossy());
        }
        None => println!("No file targeted"),
    }
}
