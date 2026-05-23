<p align="center">
  <h1 align="center">cold-cli</h1>
  <p align="center">LAMCLOD AI Coding Agent CLI</p>
  <p align="center">
    <img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square" alt="Rust">
    <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="MIT">
  </p>
</p>

---

## 简介

cold-cli 是 LAMCLOD Cold Stack 的命令行界面，一个可以读写文件、执行命令、搜索代码的 AI 编码助手。

基于 [cold-sdk](https://github.com/LamClod/cold-sdk) / [cold-context](https://github.com/LamClod/cold-context) / [cold-tools](https://github.com/LamClod/cold-tools) / [cold-agent-sdk](https://github.com/LamClod/cold-agent-sdk) 构建。

## 安装

从 [Releases](https://github.com/LamClod/cold-cli/releases) 下载对应平台的二进制，或从源码编译：

```bash
git clone https://github.com/LamClod/cold-cli.git
cd cold-cli
cargo build --release
```

## 配置

首次运行生成配置文件：

```bash
cold --init
```

编辑 `~/.cold/config.toml`：

```toml
api_key = "your-api-key"
base_url = "https://api.lamcold.com"
model = "default"
context_length = 128000
# proxy = "http://127.0.0.1:7890"
```

也支持环境变量：`COLD_API_KEY`、`COLD_BASE_URL`、`COLD_MODEL`、`HTTPS_PROXY`。

## 用法

```bash
cold
```

```
  COLD  Agent CLI
  Powered by LAMCLOD

  model: default  |  ctx: 128K

  > 读取 src/main.rs 并添加错误处理
  >> read_file {"path":"src/main.rs"}
  << read_file: 1  fn main() { ...
  -- turn 1 --
  >> edit_file {"path":"src/main.rs","old_string":"...","new_string":"..."}
  << edit_file: ok
  -- 2 turn(s) | 2 tool call(s) | 12.5K tokens
```

## 命令

| 命令 | 说明 |
|------|------|
| `/new` | 新建会话 |
| `/help` | 显示帮助 |
| `exit` | 退出 |
| `--init` | 创建默认配置 |
| `--config` | 显示配置文件路径 |

## License

MIT
