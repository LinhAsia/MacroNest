use vi::{TELEX, transform_buffer};

fn main() {
    let inputs = vec!["giair", "dd", "giải", "giai", "giair", "iair"];
    for input in inputs {
        let mut output = String::new();
        transform_buffer(&TELEX, input.chars(), &mut output);
        println!("{} -> {}", input, output);
    }
}
