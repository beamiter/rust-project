use regex::Regex;

use crate::dwm::remove_control_characters;

pub fn for_test() {
    let tt: &str = "中国";
    println!("len: {}", tt.len());
    println!("0: {}", &tt[0..3]);
    println!(
        "0:{} len: {}, decode: {}, {:0X}",
        tt.chars().nth(0).unwrap(),
        tt.chars().nth(0).unwrap().len_utf8(),
        tt.chars().nth(0).unwrap() as u32,
        tt.chars().nth(0).unwrap() as u32
    );
    println!("1: {}", &tt[3..]);
    println!(
        "1:{} len: {}, decode: {}, {:0X}",
        tt.chars().nth(1).unwrap(),
        tt.chars().nth(1).unwrap().len_utf8(),
        tt.chars().nth(1).unwrap() as u32,
        tt.chars().nth(1).unwrap() as u32
    );
    let mut text = "\u{200d}\u{2061}\u{200d}\u{2063}\u{202c}\u{202c}\u{2064}\u{2064}\u{200d}\u{202c}\u{2063}\u{202c}\u{2063}\u{2064}\u{202c}\u{200c}\u{feff}\u{feff}\u{2061}\u{2063}\u{2061}\u{200b}\u{200c}\u{feff}\u{2063}\u{200b}\u{200b}\u{200b}\u{2061}\u{200b}\u{2063}\u{200c}\u{200b}\u{200b}\u{2063}\u{2061}\u{2062}\u{200d}\u{2064}\u{feff}\u{202c}\u{2064}\u{2063}\u{200d}\u{2061}\u{200c}\u{feff}\u{202c}\u{2062}\u{202c}CP路测跟车记录 - Feishu Docs - Google Chrome";
    println!("{}", text.len());
    let binding = &remove_control_characters(&text.to_string());
    text = binding;
    println!("{}", text.len());

    let input = "^r0,7,2,4^^r2,4,22,10^^c#000000^^r3,5,20,8^^c#ffffff^^r10,5,13,8^^d^^f24^";
    // 创建一个正则表达式用于匹配 "^" 和 "^" 之间的任意字符
    let re = Regex::new(r"\^(.*?)\^").unwrap();

    // 找到所有匹配项，并收集它们到一个 Vec<String> 中
    let captured_strings: Vec<String> = re
        .captures_iter(input)
        .filter_map(|cap| cap.get(1)) // 获取第一个捕获组（括号内的部分）
        .map(|m| m.as_str().to_owned()) // 将捕获的字符串切片转换为 String
        .collect();

    println!("{:?}", captured_strings);
}
