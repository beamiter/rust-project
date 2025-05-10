use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClashConfig {
    #[serde(rename = "mixed-port", default)]
    pub mixed_port: u16,
    #[serde(rename = "redir-port", default)]
    pub redir_port: u16,
    #[serde(rename = "port", default)]
    pub port: Option<u16>,
    #[serde(rename = "socks-port", default)]
    pub socks_port: Option<u16>,
    #[serde(rename = "tproxy-port", default)]
    pub tproxy_port: Option<u16>,
    #[serde(rename = "allow-lan", default)]
    pub allow_lan: bool,
    #[serde(default)]
    pub mode: String,
    #[serde(rename = "log-level", default)]
    pub log_level: String,
    #[serde(rename = "external-controller", default)]
    pub external_controller: String,
    #[serde(rename = "external-ui", default)]
    pub external_ui: Option<String>,
    #[serde(default)]
    pub secret: Option<String>,
    // 添加 controller 字段，但在 YAML 中不存在，所以需要自定义反序列化
    #[serde(skip_deserializing)]
    pub controller: ControllerConfig,
    // 添加 DNS 配置
    #[serde(default)]
    pub dns: DnsConfig,
    // 添加代理配置
    #[serde(default)]
    pub proxies: Vec<ProxyConfig>,
    // 添加代理组配置
    #[serde(rename = "proxy-groups", default)]
    pub proxy_groups: Vec<ProxyGroupConfig>,
    // 添加规则
    #[serde(default)]
    pub rules: Vec<String>,
    // 添加 ipv6
    #[serde(default)]
    pub ipv6: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ControllerConfig {
    pub port: u16,
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DnsConfig {
    #[serde(default)]
    pub enable: bool,
    pub listen: Option<String>,
    #[serde(default)]
    pub nameserver: Vec<String>,
    #[serde(default)]
    pub fallback: Vec<String>,
    #[serde(default)]
    pub ipv6: bool,
    #[serde(rename = "enhanced-mode", default)]
    pub enhanced_mode: String,
    #[serde(rename = "fake-ip-range", default)]
    pub fake_ip_range: Option<String>,
    #[serde(rename = "fake-ip-filter", default)]
    pub fake_ip_filter: Option<Vec<String>>,
    #[serde(rename = "default-nameserver", default)]
    pub default_nameserver: Option<Vec<String>>,
    #[serde(rename = "nameserver-policy", default)]
    pub nameserver_policy: Option<HashMap<String, String>>,
    #[serde(rename = "fallback-filter", default)]
    pub fallback_filter: Option<FallbackFilterConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FallbackFilterConfig {
    #[serde(default)]
    pub geoip: bool,
    pub ipcidr: Option<Vec<String>>,
    pub domain: Option<Vec<String>>,
    #[serde(rename = "geoip-code", default)]
    pub geoip_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub proxy_type: String,
    pub server: String,
    pub port: u16,
    pub password: Option<String>,
    pub cipher: Option<String>,
    pub udp: Option<bool>,
    pub sni: Option<String>,
    pub network: Option<String>,
    #[serde(rename = "grpc-opts")]
    pub grpc_opts: Option<GrpcOptions>,
    pub alpn: Option<Vec<String>>,
    // 使用 flatten 来捕获任何其他未知字段
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcOptions {
    #[serde(rename = "grpc-service-name")]
    pub grpc_service_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyGroupConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub group_type: String,
    #[serde(default)]
    pub proxies: Vec<String>,
    pub url: Option<String>,
    pub interval: Option<u32>,
    pub strategy: Option<String>,
    // 使用 flatten 来捕获任何其他未知字段
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

impl Default for ClashConfig {
    fn default() -> Self {
        Self {
            port: Some(7890),
            socks_port: Some(7891),
            redir_port: 0,
            tproxy_port: None,
            mixed_port: 0,
            allow_lan: false,
            mode: "Rule".to_string(),
            log_level: "info".to_string(),
            external_controller: "127.0.0.1:9090".to_string(),
            external_ui: None,
            secret: Some("".to_string()),
            controller: ControllerConfig {
                port: 9090,
                secret: "".to_string(),
            },
            proxies: Vec::new(),
            proxy_groups: Vec::new(),
            rules: vec![
                "DOMAIN-SUFFIX,google.com,DIRECT".to_string(),
                "DOMAIN-KEYWORD,google,DIRECT".to_string(),
                "DOMAIN,google.com,DIRECT".to_string(),
                "DOMAIN-SUFFIX,ad.com,REJECT".to_string(),
                "GEOIP,CN,DIRECT".to_string(),
                "MATCH,DIRECT".to_string(),
            ],
            dns: DnsConfig {
                enable: true,
                listen: Some("0.0.0.0:53".to_string()),
                nameserver: vec!["114.114.114.114".to_string(), "8.8.8.8".to_string()],
                fallback: vec!["1.1.1.1".to_string()],
                ipv6: false,
                enhanced_mode: "fake-ip".to_string(),
                fake_ip_range: Some("198.18.0.1/16".to_string()),
                fake_ip_filter: None,
                default_nameserver: Some(vec![
                    "114.114.114.114".to_string(),
                    "8.8.8.8".to_string(),
                ]),
                nameserver_policy: None,
                fallback_filter: Some(FallbackFilterConfig {
                    geoip: true,
                    ipcidr: Some(vec!["240.0.0.0/4".to_string()]),
                    domain: None,
                    geoip_code: None,
                }),
            },
            ipv6: Some(false),
        }
    }
}

