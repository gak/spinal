use crate::{Skeleton, SpinalError};

pub fn parse(b: &[u8]) -> Result<Skeleton, SpinalError> {
    let skel = serde_json::from_slice(b).unwrap(); // TODO: error
    Ok(skel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_all() {
        let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.json");
        let skel = parse(b).unwrap();
        dbg!(skel);
    }
}
