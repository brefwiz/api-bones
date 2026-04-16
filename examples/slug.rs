//! Validated URL-safe slugs.
//!
//! Demonstrates `Slug::new()` validation, `Slug::from_title()` auto-conversion,
//! and all `SlugError` variants.
//!
//! Run: `cargo run --example slug`

use api_bones::{Slug, SlugError};

fn main() {
    // -- Valid slugs --
    let slug = Slug::new("hello-world").expect("valid slug");
    println!("Slug::new(\"hello-world\"): {slug}");
    println!("  as_str(): {:?}", slug.as_str());
    println!("  len():    {}", slug.len());

    // -- from_title auto-conversion --
    println!("\nSlug::from_title examples:");
    for title in [
        "Hello, World! 2026",
        "  Spaced  Out  Title  ",
        "UPPERCASE IS FINE",
        "Special: chars! @#$ everywhere",
        "",
        "!!! ???",
    ] {
        let slug = Slug::from_title(title);
        println!("  {title:40?} => {slug:?}");
    }

    // -- All error variants --
    println!("\nSlugError variants:");
    let cases: Vec<(&str, SlugError)> = vec![
        ("", SlugError::Empty),
        ("Hello", SlugError::InvalidChars),
        ("-leading", SlugError::LeadingHyphen),
        ("trailing-", SlugError::TrailingHyphen),
        ("double--hyphen", SlugError::ConsecutiveHyphens),
    ];
    let too_long = "a".repeat(129);
    for (input, _expected) in &cases {
        println!(
            "  Slug::new({input:20?}) => {:?}",
            Slug::new(input).unwrap_err()
        );
    }
    println!(
        "  Slug::new(\"aaa...\" x129) => {:?}",
        Slug::new(&too_long).unwrap_err()
    );

    // -- Conversions --
    println!("\nConversions:");
    let slug = Slug::new("my-resource").unwrap();
    let s: &str = slug.as_ref();
    println!("  AsRef<str>:   {s:?}");
    println!("  Display:      {slug}");
    println!("  into_string:  {:?}", slug.into_string());
    println!(
        "  TryFrom<&str>: {:?}",
        Slug::try_from("valid-slug").map(|s| s.to_string())
    );
}
