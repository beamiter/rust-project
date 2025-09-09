
# Table of Contents

1.  [使用方法](#org07c4647)
    1.  [构建与安装](#orgbfe6bca)
    2.  [启动守护进程](#org41e4bcd)
    3.  [控制命令](#org9c3f4b6)
    4.  [守护进程管理](#orgdc61194)
    5.  [构建并重启 JWM（与脚本行为一致）](#orgc24c616)
    6.  [调试信息](#orga39b135)


<a id="org07c4647"></a>

# 使用方法


<a id="orgbfe6bca"></a>

## 构建与安装

-   cargo build &#x2013;release
-   可将 target/release/jwm-tool 放入 PATH，例如：
    -   sudo cp target/release/jwm-tool *usr/local/bin*


<a id="org41e4bcd"></a>

## 启动守护进程

-   jwm-tool daemon
    -   指定 JWM 可执行文件路径：
    -   jwm-tool daemon &#x2013;jwm-binary /path/to/jwm
    -   或环境变量：JWM<sub>BINARY</sub>=/path/to/jwm jwm-tool daemon


<a id="org9c3f4b6"></a>

## 控制命令

-   jwm-tool start
-   jwm-tool stop
-   jwm-tool restart
-   jwm-tool status
-   jwm-tool quit


<a id="orgdc61194"></a>

## 守护进程管理

-   jwm-tool daemon-check
-   jwm-tool daemon-restart


<a id="orgc24c616"></a>

## 构建并重启 JWM（与脚本行为一致）

-   jwm-tool rebuild &#x2013;jwm-dir /path/to/jwm
-   或设置环境变量 JWM<sub>DIR</sub>


<a id="orga39b135"></a>

## 调试信息

-   jwm-tool debug

