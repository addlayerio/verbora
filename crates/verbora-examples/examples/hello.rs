//! The smallest complete program in the documentation's "Getting started".
//!
//! Run with: `cargo run -p verbora-examples --example hello`

use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    for token in WordTokenizer.tokens("Verbora tokenizes text without copying it.") {
        println!("{token}");
    }
}
