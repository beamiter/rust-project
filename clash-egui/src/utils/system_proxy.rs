use anyhow::Result;

pub struct SystemProxy;

impl SystemProxy {
    pub fn set_system_proxy(enabled: bool, http_port: u16, socks_port: u16) -> Result<()> {
        if enabled {
            Self::enable_system_proxy(http_port, socks_port)
        } else {
            Self::disable_system_proxy()
        }
    }

    #[cfg(target_os = "windows")]
    fn enable_system_proxy(http_port: u16, socks_port: u16) -> Result<()> {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let internet_settings = hkcu.open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            KEY_WRITE | KEY_READ,
        )?;

        // 设置代理服务器和绕过列表
        internet_settings.set_value("ProxyEnable", &1u32)?;
        internet_settings.set_value("ProxyServer", &format!("127.0.0.1:{}", http_port))?;
        internet_settings.set_value("ProxyOverride", &"<local>")?;

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn enable_system_proxy(http_port: u16, socks_port: u16) -> Result<()> {
        // 使用 networksetup 命令设置系统代理
        std::process::Command::new("networksetup")
            .args(&["-setwebproxy", "Wi-Fi", "127.0.0.1", &http_port.to_string()])
            .output()?;

        std::process::Command::new("networksetup")
            .args(&[
                "-setsecurewebproxy",
                "Wi-Fi",
                "127.0.0.1",
                &http_port.to_string(),
            ])
            .output()?;

        std::process::Command::new("networksetup")
            .args(&[
                "-setsocksfirewallproxy",
                "Wi-Fi",
                "127.0.0.1",
                &socks_port.to_string(),
            ])
            .output()?;

        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn enable_system_proxy(http_port: u16, socks_port: u16) -> Result<()> {
        // 对于 GNOME
        std::process::Command::new("gsettings")
            .args(&["set", "org.gnome.system.proxy", "mode", "manual"])
            .output()?;

        std::process::Command::new("gsettings")
            .args(&["set", "org.gnome.system.proxy.http", "host", "127.0.0.1"])
            .output()?;

        std::process::Command::new("gsettings")
            .args(&[
                "set",
                "org.gnome.system.proxy.http",
                "port",
                &http_port.to_string(),
            ])
            .output()?;

        std::process::Command::new("gsettings")
            .args(&["set", "org.gnome.system.proxy.https", "host", "127.0.0.1"])
            .output()?;

        std::process::Command::new("gsettings")
            .args(&[
                "set",
                "org.gnome.system.proxy.https",
                "port",
                &http_port.to_string(),
            ])
            .output()?;

        std::process::Command::new("gsettings")
            .args(&["set", "org.gnome.system.proxy.socks", "host", "127.0.0.1"])
            .output()?;

        std::process::Command::new("gsettings")
            .args(&[
                "set",
                "org.gnome.system.proxy.socks",
                "port",
                &socks_port.to_string(),
            ])
            .output()?;

        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn disable_system_proxy() -> Result<()> {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let internet_settings = hkcu.open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            KEY_WRITE | KEY_READ,
        )?;

        // 禁用代理
        internet_settings.set_value("ProxyEnable", &0u32)?;

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn disable_system_proxy() -> Result<()> {
        // 使用 networksetup 命令禁用系统代理
        std::process::Command::new("networksetup")
            .args(&["-setwebproxystate", "Wi-Fi", "off"])
            .output()?;

        std::process::Command::new("networksetup")
            .args(&["-setsecurewebproxystate", "Wi-Fi", "off"])
            .output()?;

        std::process::Command::new("networksetup")
            .args(&["-setsocksfirewallproxystate", "Wi-Fi", "off"])
            .output()?;

        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn disable_system_proxy() -> Result<()> {
        // 对于 GNOME
        std::process::Command::new("gsettings")
            .args(&["set", "org.gnome.system.proxy", "mode", "none"])
            .output()?;

        Ok(())
    }
}
