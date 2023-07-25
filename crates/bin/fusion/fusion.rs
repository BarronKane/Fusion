use std::env;

fn main() {
    scoped_main();
}

fn scoped_main() {
    println!("Hello, world!");

    let _args: Vec<String> = env::args().collect();

    
}
