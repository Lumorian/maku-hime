//! 独立运行的 maku-hime 服务（不开桌面窗口）。
//!
//! 用途：在 OBS / 浏览器里用弹幕墙，或把 maku-hime 当成本地弹幕 API。
//! 桌面端（Tauri）会在进程内起同一个服务，不需要额外启动它。

use std::process::ExitCode;

use makuhime_core::config::AppConfig;
use makuhime_core::engine::Engine;
use tracing_subscriber::EnvFilter;

struct Args {
    host: Option<String>,
    port: Option<u16>,
    rooms: Vec<u64>,
    ignore_config_rooms: bool,
    open_browser: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        host: None,
        port: None,
        rooms: Vec::new(),
        ignore_config_rooms: false,
        open_browser: false,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("maku-hime-server {}", makuhime_core::VERSION);
                std::process::exit(0);
            }
            "--host" => args.host = Some(iter.next().ok_or("--host 缺少参数")?),
            "--port" | "-p" => {
                let value = iter.next().ok_or("--port 缺少参数")?;
                args.port = Some(
                    value
                        .parse()
                        .map_err(|_| format!("端口不是数字: {value}"))?,
                );
            }
            "--room" | "-r" => {
                let value = iter.next().ok_or("--room 缺少参数")?;
                let id: u64 = value
                    .parse()
                    .map_err(|_| format!("房间号不是数字: {value}"))?;
                args.rooms.push(id);
            }
            "--no-rooms" => args.ignore_config_rooms = true,
            "--open" => args.open_browser = true,
            other => return Err(format!("未知参数: {other}（--help 查看用法）")),
        }
    }
    Ok(args)
}

fn print_help() {
    println!(
        "maku-hime-server {}\n\n\
用法: maku-hime-server [选项]\n\n\
选项:\n  \
  -h, --help          显示帮助\n  \
  -V, --version       显示版本\n  \
      --host <HOST>   监听地址（默认 127.0.0.1）\n  \
  -p, --port <PORT>   监听端口（默认 3939，被占用时自动顺延）\n  \
  -r, --room <ID>     订阅房间号，可重复\n  \
      --no-rooms      忽略配置文件里保存的自动订阅房间\n  \
      --open          启动后用浏览器打开面板\n\n\
环境变量:\n  \
  MAKUHIME_PORT / MAKUHIME_HOST   覆盖监听地址\n  \
  MAKUHIME_WEB_DIR                指定前端构建产物目录\n  \
  MAKUHIME_API_TOKEN              指定访问令牌（默认自动生成，存在配置目录的 api-token）\n  \
  MAKUHIME_SYSTEM_PROXY=1         允许走系统代理（默认直连，切代理不会断弹幕）\n  \
  RUST_LOG                        日志级别（默认 info）",
        makuhime_core::VERSION
    );
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let args = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("参数错误: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut config = AppConfig::load();
    if args.ignore_config_rooms {
        config.rooms.clear();
    }
    let host = args
        .host
        .or_else(|| std::env::var("MAKUHIME_HOST").ok())
        .unwrap_or_else(|| config.server.host.clone());
    let port = args
        .port
        .or_else(|| {
            std::env::var("MAKUHIME_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(config.server.port);

    let engine = match Engine::start(config).await {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("启动失败: {err}");
            return ExitCode::FAILURE;
        }
    };

    for room in args.rooms {
        match engine.add_room(room).await {
            Ok(state) => tracing::info!(room = room, title = %state.info.title, "已订阅"),
            Err(err) => tracing::warn!(room, %err, "订阅失败"),
        }
    }

    let addr = match makuhime_server::serve(engine.clone(), &host, port).await {
        Ok(addr) => addr,
        Err(err) => {
            eprintln!("监听 {host}:{port} 失败: {err}");
            return ExitCode::FAILURE;
        }
    };

    let url = format!("http://{addr}");
    println!("maku-hime {} 已启动", makuhime_core::VERSION);
    println!("  控制面板: {url}");
    println!("  OBS 弹幕墙: {url}/obs?room=<房间号>");
    println!("  健康检查: {url}/api/health");
    println!(
        "  访问令牌: {}（/api 接口需要它，用浏览器打开上面的地址会自动带上）",
        makuhime_server::auth::token_path().display()
    );

    if args.open_browser || std::env::var("MAKUHIME_OPEN").is_ok() {
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }

    tokio::signal::ctrl_c().await.ok();
    println!("\n正在退出…");
    engine.shutdown().await;
    ExitCode::SUCCESS
}
