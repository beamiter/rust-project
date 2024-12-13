use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::{self};
use serde_json::Value;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use tokio;

fn extract_version(url: &str) -> Option<String> {
    // 定义一个正则表达式来查找版本号
    let version_regex = Regex::new(r"releases/download/(.*?)/").unwrap();

    // 在 URL 中搜索匹配的版本号
    version_regex
        .captures(url)
        .and_then(|caps| caps.get(1).map(|match_| match_.as_str().to_string()))
}
async fn post_process_neovim(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file_path = "/tmp/neovim.appimage";

    // 发送HEAD请求以获取文件大小
    let client = reqwest::Client::new();
    let response = client.head(url).send().await?;
    let total_size = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|ct_len| ct_len.to_str().ok())
        .and_then(|ct_len| ct_len.parse::<u64>().ok())
        .unwrap_or(0);

    // 创建进度条
    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
            .progress_chars("#>-"),
    );

    // 开始下载文件
    let mut request = client.get(url).send().await?;
    let mut file = File::create(file_path)?;
    let mut downloaded: u64 = 0; // 已下载的字节数
    while let Some(chunk) = request.chunk().await? {
        file.write_all(&chunk)?;
        let new = downloaded + chunk.len() as u64;
        downloaded = new;
        pb.set_position(new);
    }
    pb.finish_with_message("finish");

    // 设置可执行权限(chmod +x)
    let mut perms = file.metadata()?.permissions();
    perms.set_mode(0o755); // 类似于 chmod 755
    file.set_permissions(perms)?;

    println!("Neovim AppImage here: {}", file_path);

    Ok(())
}

async fn post_process_vim(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file_path = "/tmp/vim.appimage";

    // 发送HEAD请求以获取文件大小
    let client = reqwest::Client::new();
    let response = client.head(url).send().await?;
    let total_size = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|ct_len| ct_len.to_str().ok())
        .and_then(|ct_len| ct_len.parse::<u64>().ok())
        .unwrap_or(0);

    // 创建进度条
    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
            .progress_chars("#>-"),
    );

    // 开始下载文件
    let mut request = client.get(url).send().await?;
    let mut file = File::create(file_path)?;
    let mut downloaded: u64 = 0; // 已下载的字节数
    while let Some(chunk) = request.chunk().await? {
        file.write_all(&chunk)?;
        let new = downloaded + chunk.len() as u64;
        downloaded = new;
        pb.set_position(new);
    }
    pb.finish_with_message("下载完成");

    // 设置可执行权限(chmod +x)
    let mut perms = file.metadata()?.permissions();
    perms.set_mode(0o755); // 类似于 chmod 755
    file.set_permissions(perms)?;

    println!("Vim AppImage here: {}", file_path);

    Ok(())
}

async fn down_vim(vim_version_map: &mut HashMap<String, String>) -> Result<(), Box<dyn Error>> {
    println!("down vim");
    // GitHub API URL for the releases of the vim-appimage repository
    let url = "https://api.github.com/repos/vim/vim-appimage/releases";

    // 发送请求并获取JSON响应数据
    let response = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "request")
        .send()
        .await?;

    // 将响应解析为JSON
    let releases: Value = response.json().await?;

    // 遍历每个发布版本，查找指定的AppImage资源
    if let Some(ref release) = releases.as_array() {
        for item in &release[0..10] {
            if let Some(assets) = item["assets"].as_array() {
                for asset in assets {
                    let name = asset["name"].as_str().unwrap_or_default();
                    // 寻找特定的仓库资产名称
                    if name.starts_with("Vim") && name.ends_with("AppImage") {
                        let download_url = asset["browser_download_url"].to_string();
                        let url = download_url.trim_matches('\"');
                        match extract_version(url) {
                            Some(version) => {
                                // println!("Found version: {} in URL: {}", version, url);
                                vim_version_map.insert(version, url.to_string());
                            }
                            None => println!("No version found in URL: {}", url),
                        }
                        // return post_process_vim(&url).await;
                    }
                }
            }
        }
    } else {
        eprintln!("Error parsing release information.");
    }

    Ok(())
}

async fn down_nvim(nvim_version_map: &mut HashMap<String, String>) -> Result<(), Box<dyn Error>> {
    println!("down neovim");
    // GitHub API URL for the releases of the vim-appimage repository
    // let url = "https://github.com/neovim/neovim/releases";
    let url = "https://api.github.com/repos/neovim/neovim/releases";

    // 发送请求并获取JSON响应数据
    let response = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "request")
        .send()
        .await?;

    // 将响应解析为JSON
    let releases: Value = response.json().await?;

    // 遍历每个发布版本，查找指定的AppImage资源
    if let Some(ref release) = releases.as_array() {
        for item in &release[0..10] {
            if let Some(assets) = item["assets"].as_array() {
                for asset in assets {
                    let name = asset["name"].as_str().unwrap_or_default();
                    if name == "nvim.appimage" {
                        let download_url = asset["browser_download_url"].to_string();
                        let url = download_url.trim_matches('\"');
                        match extract_version(url) {
                            Some(version) => {
                                // println!("Found version: {} in URL: {}", version, url);
                                nvim_version_map.insert(version, url.to_string());
                            }
                            None => println!("No version found in URL: {}", url),
                        }
                        // return post_process_neovim(&url).await;
                    }
                }
            }
        }
    } else {
        eprintln!("Error parsing release information.");
    }

    Ok(())
}

async fn install_menu(choice: i32, version_map: &HashMap<String, String>) {
    let version = prompt("enter version: ");
    let url = version_map.get(&version);
    if url.is_none() {
        print!("no match version!!!");
        return;
    }
    let url = url.unwrap();
    match choice {
        0 => {
            let _ = post_process_vim(&url).await;
        }
        1 => {
            let _ = post_process_neovim(&url).await;
        }
        _ => {}
    }
}

// 0 for vim, 1 for neovim
async fn sub_menu(choice: i32) {
    let mut version_map: HashMap<String, String> = HashMap::new();
    match choice {
        0 => {
            let _ = down_vim(&mut version_map).await;
        }
        1 => {
            let _ = down_nvim(&mut version_map).await;
        }
        _ => {
            print!("Invalid");
            return;
        }
    }

    loop {
        match prompt("0: list; 1: install; 2: quit\nplease enter: ").as_str() {
            "0" => {
                let keys: Vec<&str> = version_map.keys().map(|s| s.as_str()).collect();
                let joined_keys = keys.join(" | ");
                println!("{}", joined_keys);
            }
            "1" => {
                install_menu(choice, &version_map).await;
                break;
            }
            "2" => {
                break;
            }
            _ => {
                println!("error!!! please re-input")
            }
        }
    }
}

async fn main_menu() {
    loop {
        println!("welcome to use bob");
        println!("A vim and neovim version manager");
        println!("1. for vim");
        println!("2. for nvim");
        println!("3. quit");

        match prompt("please enter: ").as_str() {
            "1" => {
                sub_menu(0).await;
            }
            "2" => {
                sub_menu(1).await;
            }
            "3" => {
                print!("wish you a good day");
                break;
            }
            _ => {
                println!("error!!! please re-input")
            }
        }
    }
}

fn prompt(message: &str) -> String {
    print!("{}", message);
    io::stdout().flush().unwrap();

    let mut res: String = String::new();
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => {
            res = input.trim().to_owned();
        }
        Err(err) => println!("input error: {}", err),
    }
    res
}

#[tokio::main]
async fn main() {
    main_menu().await;
}
