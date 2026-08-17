//! The smallest complete program in the documentation's "Getting started".
//!
//! Run with: `cargo run -p verbora-examples --example hello`

use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let tokenizer = AggressiveTokenizer::new();

    for token in tokenizer.tokens("Verbora tokenizes text without copying it.") {
        println!("{token}");
    }
}
