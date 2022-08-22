pub fn crate_prefix(name: &str) -> String {
    match name.len() {
        1 => "1".to_owned(),
        2 => "2".to_owned(),
        3 => format!("3/{}", name.chars().next().unwrap()),
        _ => {
            let chars: Vec<_> = name.chars().take(4).collect();
            format!("{}{}/{}{}", chars[0], chars[1], chars[2], chars[3])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_prefix_samples() {
        assert_eq!(crate_prefix("a"), "1");
        assert_eq!(crate_prefix("ab"), "2");
        assert_eq!(crate_prefix("abc"), "3/a");
        assert_eq!(crate_prefix("cargo"), "ca/rg");
    }
}
