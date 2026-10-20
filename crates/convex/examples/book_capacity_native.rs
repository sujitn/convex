//! Native parity companion for tools/core_capacity_workflow.py.
use std::{
    error::Error,
    io::{self, Read},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut raw = String::new();
    io::stdin()
        .take(2 * 1024 * 1024 + 1)
        .read_to_string(&mut raw)?;
    if raw.len() > 2 * 1024 * 1024 {
        return Err(io::Error::other("book request exceeds2MiB").into());
    }
    println!("{}", convex::library::execute_book_json(&raw));
    Ok(())
}
