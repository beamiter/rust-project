#[cfg(test)]

mod tests {

    use config::{dmenucmd, termcmd};

    use crate::*;

    #[test]
    fn test_spawn() {
        let mut arg = dwm::Arg::V(termcmd.clone());
        dwm::spawn(&mut arg);
        let mut arg = dwm::Arg::V(dmenucmd.clone());
        dwm::spawn(&mut arg);
    }
}
