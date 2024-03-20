const PCRE2_CODE_UNIT_WIDTH: i8 = 8;

enum ConfigItemType {
    STRING,
    STRINGLIST,
    BOOLEAN,
    INT64,
    UINT64,
}

struct ConfigItem {
    s: Vec<char>,
    n: Vec<char>,
    t: ConfigItemType,
    l: u64,
}

fn main() {
    println!("Hello, world!");
}
