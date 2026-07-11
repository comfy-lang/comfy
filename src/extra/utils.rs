use rand::Rng;

use crate::frontend::tokenizer::Token;

fn generate_random_alphanumeric_string(length: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                             abcdefghijklmnopqrstuvwxyz\
                             0123456789";
    let mut rng = rand::rng();
    (0..length)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

pub fn _generate_num_varname() -> String {
    let mut name = String::new();
    name.push_str("int_");
    name.push_str(&generate_random_alphanumeric_string(8));
    name
}

pub fn generate_str_varname() -> String {
    let mut name = String::new();
    name.push_str("str_");
    name.push_str(&generate_random_alphanumeric_string(8));
    name
}

pub fn get_bytes_from_type(token: &Token) -> usize {
    match token {
        Token::Bool => 1,
        Token::Char => 1,
        Token::Int8 => 1,
        Token::Int16 => 2,
        Token::Int32 => 4,
        Token::Str => 4, // Assuming a pointer size for strings
        _ => panic!("Unsupported type for byte size calculation: {:?}", token),
    }
}
