//! 弹幕语音播报。
//!
//! 三种后端：
//! - webview：把文本推给前端，用浏览器 speechSynthesis 朗读（零依赖，音色取决于系统）；
//! - command：调用本机命令行 TTS（piper / espeak-ng / spd-say / ekho …），配置里写 argv，
//!   {text} 会被替换成播报文本；
//! - auto：按可用性自动挑一个 command 后端。
//!
//! 队列有上限，弹幕洪水时宁可丢弃也不堆积。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tokio::sync::{RwLock, mpsc};

use crate::config::{AppConfig, TtsProvider};

/// 一条播报任务。
#[derive(Debug, Clone, Serialize)]
pub struct TtsJob {
    pub text: String,
    /// chat / gift / guard / super_chat / test
    pub kind: String,
    /// 实际使用的后端描述
    pub backend: String,
}

#[derive(Clone)]
pub struct TtsHandle {
    tx: mpsc::Sender<TtsJob>,
    dropped: Arc<AtomicU64>,
    backend: Arc<RwLock<String>>,
}

impl TtsHandle {
    pub async fn backend(&self) -> String {
        self.backend.read().await.clone()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// 尝试入队；队列满时丢弃并计数。
    pub fn speak(&self, kind: &str, text: impl Into<String>) -> bool {
        let text = sanitize(&text.into());
        if text.is_empty() {
            return false;
        }
        let job = TtsJob {
            text,
            kind: kind.to_string(),
            backend: String::new(),
        };
        match self.tx.try_send(job) {
            Ok(()) => true,
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }
}

/// 启动播报工作线程；返回句柄与一个接收端，用于把 webview 播报推给前端。
pub fn spawn(config: Arc<RwLock<AppConfig>>, out: mpsc::UnboundedSender<TtsJob>) -> TtsHandle {
    let (tx, mut rx) = mpsc::channel::<TtsJob>(64);
    let dropped = Arc::new(AtomicU64::new(0));
    let backend = Arc::new(RwLock::new("off".to_string()));
    let backend_for_task = backend.clone();
    tokio::spawn(async move {
        // 启动时探测一次默认后端
        {
            let tts_config = config.read().await.tts.clone();
            let resolved = resolve_backend(&tts_config).await;
            *backend_for_task.write().await = resolved;
        }
        while let Some(mut job) = rx.recv().await {
            let cfg = config.read().await.tts.clone();
            if !cfg.enabled {
                continue;
            }
            let provider = effective_provider(&cfg);
            let resolved = resolve_backend(&cfg).await;
            *backend_for_task.write().await = resolved.clone();
            job.backend = resolved.clone();
            match provider {
                TtsProvider::Off => continue,
                TtsProvider::Webview => {
                    let _ = out.send(job);
                }
                TtsProvider::Command | TtsProvider::Auto => {
                    if let Some(argv) = command_line(&cfg, &resolved) {
                        run_command(&argv, &job.text).await;
                    }
                    let _ = out.send(job);
                }
            }
        }
    });

    TtsHandle {
        tx,
        dropped,
        backend,
    }
}

fn effective_provider(cfg: &crate::config::TtsConfig) -> TtsProvider {
    match cfg.provider {
        TtsProvider::Auto => {
            if cfg.command.is_empty() {
                TtsProvider::Auto
            } else {
                TtsProvider::Command
            }
        }
        other => other,
    }
}

/// 解析实际使用的后端描述：webview / command:<程序> / off
pub async fn resolve_backend(cfg: &crate::config::TtsConfig) -> String {
    match effective_provider(cfg) {
        TtsProvider::Off => "off".into(),
        TtsProvider::Webview => "webview".into(),
        TtsProvider::Command => cfg
            .command
            .first()
            .map(|program| format!("command:{program}"))
            .unwrap_or_else(|| "off".into()),
        TtsProvider::Auto => match detect_command().await {
            Some(argv) => format!("command:{}", argv[0]),
            None => "off".into(),
        },
    }
}

fn command_line(cfg: &crate::config::TtsConfig, resolved: &str) -> Option<Vec<String>> {
    if !cfg.command.is_empty() {
        return Some(cfg.command.clone());
    }
    if let Some(program) = resolved.strip_prefix("command:") {
        let mut argv = vec![program.to_string(), "{text}".to_string()];
        if program == "espeak-ng" {
            argv = vec![
                program.to_string(),
                "-v".into(),
                "zh".into(),
                "{text}".into(),
            ];
        }
        return Some(argv);
    }
    None
}

/// 按可用性挑一个命令行 TTS。
async fn detect_command() -> Option<Vec<String>> {
    const CANDIDATES: &[(&str, &[&str])] = &[
        ("edge-playback", &["--text", "{text}"]),
        ("ekho", &["{text}"]),
        ("espeak-ng", &["-v", "zh", "{text}"]),
        ("spd-say", &["{text}"]),
    ];
    for (program, args) in CANDIDATES {
        if which(program).is_some() {
            let mut argv = vec![program.to_string()];
            argv.extend(args.iter().map(|arg| arg.to_string()));
            return Some(argv);
        }
    }
    None
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

async fn run_command(argv: &[String], text: &str) {
    let Some((program, args)) = argv.split_first() else {
        return;
    };
    let has_placeholder = args.iter().any(|arg| arg.contains("{text}"));
    let mut command = tokio::process::Command::new(program);
    for arg in args {
        command.arg(arg.replace("{text}", text));
    }
    if !has_placeholder {
        command.arg(text);
    }
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());
    match tokio::time::timeout(std::time::Duration::from_secs(30), command.status()).await {
        Ok(Ok(status)) if status.success() => {}
        Ok(Ok(status)) => tracing::warn!(%status, program, "TTS 命令退出码非 0"),
        Ok(Err(err)) => tracing::warn!(%err, program, "TTS 命令启动失败"),
        Err(_) => tracing::warn!(program, "TTS 命令超时"),
    }
}

/// 清掉表情标记、链接与多余空白，避免朗读出噪音。
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '[' => {
                // 吃掉 [xxx] 这种表情标记
                let mut buffer = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == ']' {
                        closed = true;
                        break;
                    }
                    buffer.push(inner);
                    if buffer.chars().count() > 16 {
                        break;
                    }
                }
                if !closed {
                    out.push('[');
                    out.push_str(&buffer);
                }
            }
            'h' if out.is_empty() || out.ends_with(' ') => {
                // 粗略过滤 http(s) 链接
                let mut buffer = String::from("h");
                while let Some(next) = chars.peek().copied() {
                    if next.is_whitespace() {
                        break;
                    }
                    buffer.push(next);
                    chars.next();
                }
                if !buffer.starts_with("http") {
                    out.push_str(&buffer);
                }
            }
            _ => out.push(ch),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 截断到 max_chars 个字符。
pub fn truncate(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_emoticon_tokens_and_urls() {
        assert_eq!(sanitize("你好[大笑]世界"), "你好世界");
        assert_eq!(sanitize("看看 https://example.com/a 这个"), "看看 这个");
        assert_eq!(sanitize("  多余   空格  "), "多余 空格");
        assert_eq!(sanitize("[不完整的括号"), "[不完整的括号");
    }

    #[test]
    fn truncates_long_text() {
        assert_eq!(truncate("一二三四五", 3), "一二三…");
        assert_eq!(truncate("一二三", 3), "一二三");
    }

    #[test]
    fn resolves_off_backend() {
        let cfg = crate::config::TtsConfig {
            provider: TtsProvider::Off,
            ..Default::default()
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(runtime.block_on(resolve_backend(&cfg)), "off");
    }
}
