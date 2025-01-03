use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::{self};
use serde_json::Value;
use std::fs::File;
use std::io::{self, ErrorKind, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tokio;

struct VersionInfo {
    version: String,
    url: String,
}

fn find_version<'a>(versions: &'a Vec<VersionInfo>, query: &str) -> Option<&'a VersionInfo> {
    versions.iter().find(|&vi| vi.version == query)
}

fn extract_version(url: &str) -> Option<String> {
    // 定义一个正则表达式来查找版本号
    let version_regex = Regex::new(r"releases/download/(.*?)/").unwrap();

    // 在 URL 中搜索匹配的版本号
    version_regex
        .captures(url)
        .and_then(|caps| caps.get(1).map(|match_| match_.as_str().to_string()))
}

fn move_file(file_path: &str, target_path: &str) -> io::Result<()> {
    print!("正在移动文件... ");
    io::stdout().flush()?;
    let mv_output = Command::new("sudo")
        .args(["mv", file_path, target_path])
        .output()?;
    if !mv_output.status.success() {
        println!("失败");
        return Err(io::Error::new(
            ErrorKind::Other,
            format!(
                "移动文件失败: {}",
                String::from_utf8_lossy(&mv_output.stderr)
            ),
        ));
    }
    println!("成功");
    print!("设置文件权限... ");
    io::stdout().flush()?;
    let chmod_output = Command::new("sudo")
        .args(["chmod", "755", target_path])
        .output()?;
    if !chmod_output.status.success() {
        println!("失败");
        return Err(io::Error::new(
            ErrorKind::Other,
            format!(
                "设置权限失败: {}",
                String::from_utf8_lossy(&chmod_output.stderr)
            ),
        ));
    }
    println!("成功");
    Ok(())
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

    let target_path = "/usr/local/bin/nvim";
    let _ = move_file(file_path, target_path);

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

    let target_path = "/usr/local/bin/vim";
    let _ = move_file(file_path, target_path);

    Ok(())
}

async fn down_vim(
    vim_version_vec: &mut Vec<VersionInfo>,
) -> Result<(), Box<dyn std::error::Error>> {
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
                                vim_version_vec.push(VersionInfo {
                                    version,
                                    url: url.to_string(),
                                });
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

async fn down_nvim(
    nvim_version_vec: &mut Vec<VersionInfo>,
) -> Result<(), Box<dyn std::error::Error>> {
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
                                nvim_version_vec.push(VersionInfo {
                                    version,
                                    url: url.to_string(),
                                });
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

async fn install_menu(choice: i32, url: &String) {
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
    let mut version_vec: Vec<VersionInfo> = Vec::new();
    match choice {
        0 => {
            let _ = down_vim(&mut version_vec).await;
        }
        1 => {
            let _ = down_nvim(&mut version_vec).await;
        }
        _ => {
            print!("Invalid");
            return;
        }
    }

    loop {
        println!("list and install, 0 for quit");
        let joined_versions = version_vec
            .iter()
            .map(|vi| vi.version.clone()) // or map(|vi| &vi.version) if you don't need to own the strings
            .collect::<Vec<String>>() // Collect into a vector of Strings
            .join(" | "); // Join all strings in the vector with " | "
        println!("{}", joined_versions);
        let version = prompt("enter version: ");
        if version == "0" {
            break;
        }
        let version_info = find_version(&version_vec, &version);
        if let Some(ref version_info) = version_info {
            install_menu(choice, &version_info.url).await;
            println!("Install finished");
            break;
        } else {
            print!("no match version!!!");
            continue;
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
