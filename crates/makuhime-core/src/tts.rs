//! 弹幕语音播报。
//!
//! 四种后端：
//! - webview：把文本推给前端，用浏览器 speechSynthesis 朗读（零依赖，音色取决于系统）；
//! - command：调用本机命令行 TTS（piper / espeak-ng / spd-say / ekho …），配置里写 argv，
//!   {text} 会被替换成播报文本；
//! - auto：按可用性自动挑一个 command 后端；
//! - mimo：请求小米 MiMo 云端 TTS 生成 WAV，再用本机播放器播放（参考 dmnotifier）。
//!
//! 队列有上限，弹幕洪水时宁可丢弃也不堆积。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{RwLock, mpsc};

use crate::config::{AppConfig, MimoConfig, TtsConfig, TtsProvider};

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
    tx: mpsc::UnboundedSender<TtsJob>,
    /// 已入队但还没播完的条数
    pending: Arc<AtomicUsize>,
    /// 队列上限，跟随配置里的 tts.max_queue
    limit: Arc<AtomicUsize>,
    dropped: Arc<AtomicU64>,
    backend: Arc<RwLock<String>>,
}

impl TtsHandle {
    pub async fn backend(&self) -> String {
        self.backend.read().await.clone()
    }

    /// 配置变化后刷新设置页展示的后端名（不必等下一次播报）。
    pub async fn set_backend(&self, label: impl Into<String>) {
        *self.backend.write().await = label.into();
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// 尝试入队；排队数达到 max_queue 时丢弃并计数，避免弹幕洪水时无限堆积。
    pub fn speak(&self, kind: &str, text: impl Into<String>) -> bool {
        let text = sanitize(&text.into());
        if text.is_empty() {
            return false;
        }
        if self.pending.load(Ordering::Relaxed) >= self.limit.load(Ordering::Relaxed) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let job = TtsJob {
            text,
            kind: kind.to_string(),
            backend: String::new(),
        };
        self.pending.fetch_add(1, Ordering::Relaxed);
        if self.tx.send(job).is_err() {
            self.pending.fetch_sub(1, Ordering::Relaxed);
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        true
    }
}

/// 解析出来的后端：展示名 + 真正的 argv（自动探测时不能只留下程序名）。
struct ResolvedBackend {
    label: String,
    argv: Option<Vec<String>>,
}

/// 启动播报工作线程；返回句柄与一个接收端，用于把 webview 播报推给前端。
///
/// `notices` 接收后端失败原因（例如 MiMo 没配 API Key / 网络错误），由引擎转成界面提示。
pub fn spawn(
    config: Arc<RwLock<AppConfig>>,
    out: mpsc::UnboundedSender<TtsJob>,
    notices: mpsc::UnboundedSender<String>,
) -> TtsHandle {
    let (tx, mut rx) = mpsc::unbounded_channel::<TtsJob>();
    let dropped = Arc::new(AtomicU64::new(0));
    let backend = Arc::new(RwLock::new("off".to_string()));
    let pending = Arc::new(AtomicUsize::new(0));
    let limit = Arc::new(AtomicUsize::new(32));

    let backend_for_task = backend.clone();
    let pending_for_task = pending.clone();
    let limit_for_task = limit.clone();
    tokio::spawn(async move {
        // 启动时探测一次默认后端与队列上限
        {
            let tts_config = config.read().await.tts.clone();
            let resolved = resolve(&tts_config).await;
            *backend_for_task.write().await = resolved.label;
            limit_for_task.store(tts_config.max_queue.max(1), Ordering::Relaxed);
        }
        while let Some(mut job) = rx.recv().await {
            pending_for_task.fetch_sub(1, Ordering::Relaxed);
            let cfg = config.read().await.tts.clone();
            limit_for_task.store(cfg.max_queue.max(1), Ordering::Relaxed);
            // 「试听」不受总开关影响，否则关掉播报后无法在设置里调后端
            if !cfg.enabled && job.kind != "test" {
                continue;
            }
            let resolved = resolve(&cfg).await;
            *backend_for_task.write().await = resolved.label.clone();
            job.backend = resolved.label;
            match cfg.provider {
                TtsProvider::Off => continue,
                TtsProvider::Webview => {
                    let _ = out.send(job);
                }
                TtsProvider::Command | TtsProvider::Auto => {
                    if let Some(argv) = &resolved.argv {
                        run_command(argv, &job.text).await;
                    }
                    let _ = out.send(job);
                }
                TtsProvider::Mimo => {
                    if let Err(err) = speak_mimo(&cfg, &job.text).await {
                        tracing::warn!(%err, "MiMo 语音播报失败");
                        let _ = notices.send(format!("语音播报失败：{err}"));
                    }
                    let _ = out.send(job);
                }
            }
        }
    });

    TtsHandle {
        tx,
        pending,
        limit,
        dropped,
        backend,
    }
}

/// 解析实际使用的后端：展示名 + 完整 argv。
async fn resolve(cfg: &TtsConfig) -> ResolvedBackend {
    match cfg.provider {
        TtsProvider::Off => ResolvedBackend {
            label: "off".into(),
            argv: None,
        },
        TtsProvider::Mimo => ResolvedBackend {
            label: if cfg.mimo.configured() {
                format!("mimo:{}", cfg.mimo.voice)
            } else {
                "mimo:未配置 api_key".into()
            },
            argv: None,
        },
        TtsProvider::Webview => ResolvedBackend {
            label: "webview".into(),
            argv: None,
        },
        TtsProvider::Command => {
            if cfg.command.is_empty() {
                ResolvedBackend {
                    label: "off".into(),
                    argv: None,
                }
            } else {
                ResolvedBackend {
                    label: format!("command:{}", cfg.command[0]),
                    argv: Some(cfg.command.clone()),
                }
            }
        }
        TtsProvider::Auto => {
            // 用户显式写了命令行就按命令行来，否则自动探测
            if !cfg.command.is_empty() {
                return ResolvedBackend {
                    label: format!("command:{}", cfg.command[0]),
                    argv: Some(cfg.command.clone()),
                };
            }
            match detect_command().await {
                Some(argv) => ResolvedBackend {
                    label: format!("command:{}", argv[0]),
                    argv: Some(argv),
                },
                None => ResolvedBackend {
                    label: "off".into(),
                    argv: None,
                },
            }
        }
    }
}

/// 只要后端描述（设置页展示用）。
pub async fn resolve_backend(cfg: &TtsConfig) -> String {
    resolve(cfg).await.label
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

/// 请求小米 MiMo 生成一条 WAV 并播放。
///
/// 请求/响应格式与 dmnotifier 的 tts 插件保持一致：
/// POST {base_url}/chat/completions，头 `api-key`，assistant 单条消息，
/// 返回的 WAV 是 base64 放在 choices[0].message.audio.data。
async fn speak_mimo(cfg: &TtsConfig, text: &str) -> Result<(), String> {
    let audio = generate_mimo_audio(&cfg.mimo, text).await?;
    let file = TempAudio::write(&audio).await?;
    play_wav(file.path()).await
}

/// 设置页「试听」用的同步路径：立即请求、播放，并把失败原因返回给接口。
pub async fn speak_mimo_now(cfg: &TtsConfig, text: &str) -> Result<String, String> {
    speak_mimo(cfg, text).await?;
    Ok(format!("mimo:{}", cfg.mimo.voice))
}

async fn generate_mimo_audio(mimo: &MimoConfig, text: &str) -> Result<Vec<u8>, String> {
    if !mimo.configured() {
        return Err("还没有配置小米 MiMo 的 API Key".into());
    }
    // 最关键的一条：api-key 只能发给我们校验过的公网 https 地址，
    // 否则「改一下 base_url 再点试听」就能把密钥送到任意主机。
    if !mimo.base_url_allowed() {
        return Err("MiMo 服务地址必须是公网 https 地址（不能指向本机或内网）".into());
    }
    // 和 B 站请求用同一套代理策略：不吃系统代理（见 http::client_builder）
    let client = crate::http::client_builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败: {err}"))?;
    let body = serde_json::json!({
        "model": mimo.model,
        "messages": [{ "role": "assistant", "content": text }],
        "audio": { "format": "wav", "voice": mimo.voice },
    });
    let response = client
        .post(mimo.endpoint())
        .header("api-key", mimo.api_key.trim())
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("请求 MiMo 失败: {err}"))?;
    let status = response.status();
    let text_body = response
        .text()
        .await
        .map_err(|err| format!("读取 MiMo 响应失败: {err}"))?;
    if !status.is_success() {
        // 响应里可能带服务端错误详情；只取一小段，避免把整段音频/隐私写进日志
        let detail = error_message(&text_body);
        return Err(format!("MiMo 返回 HTTP {status}{detail}"));
    }
    let value: Value =
        serde_json::from_str(&text_body).map_err(|err| format!("解析 MiMo 响应失败: {err}"))?;
    parse_mimo_audio(&value)
}

/// 从 MiMo 的 JSON 响应里取出 base64 WAV 并解码。
fn parse_mimo_audio(value: &Value) -> Result<Vec<u8>, String> {
    if let Some(message) = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        && !message.is_empty()
    {
        return Err(format!("MiMo 返回错误: {}", truncate_detail(message)));
    }
    let data = value
        .pointer("/choices/0/message/audio/data")
        .and_then(Value::as_str)
        .filter(|data| !data.is_empty())
        .ok_or_else(|| "MiMo 返回里没有音频数据".to_string())?;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|err| format!("音频 base64 解码失败: {err}"))
}

fn error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && let Some(message) = value
            .pointer("/error/message")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
        && !message.is_empty()
    {
        return format!("：{}", truncate_detail(message));
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("：{}", truncate_detail(trimmed))
    }
}

fn truncate_detail(text: &str) -> String {
    let trimmed: String = text.chars().take(160).collect();
    if text.chars().count() > 160 {
        format!("{trimmed}…")
    } else {
        trimmed
    }
}

/// 播报用的临时 WAV：离开作用域时删除。
struct TempAudio {
    path: PathBuf,
}

impl TempAudio {
    async fn write(bytes: &[u8]) -> Result<Self, String> {
        use std::io::Write;

        let dir = std::env::temp_dir();
        let mut last_err = String::new();
        // 随机文件名 + create_new（O_EXCL）：临时目录是共享的，
        // 别让人提前放一个同名符号链接把文件写到别的地方去。
        for _ in 0..8 {
            let name = format!("maku-hime-tts-{}.wav", uuid::Uuid::new_v4().simple());
            let path = dir.join(name);
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    file.write_all(bytes)
                        .map_err(|err| format!("写入临时音频失败: {err}"))?;
                    return Ok(Self { path });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    last_err = err.to_string();
                }
                Err(err) => return Err(format!("写入临时音频失败: {err}")),
            }
        }
        Err(format!("写入临时音频失败: {last_err}"))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempAudio {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 依次尝试系统里常见的音频播放器。mpv / ffplay 与 dmnotifier 保持一致，
/// 再补 paplay / pw-play / aplay / afplay，覆盖没装 mpv 的桌面。
async fn play_wav(path: &Path) -> Result<(), String> {
    const PLAYERS: &[(&str, &[&str])] = &[
        ("mpv", &["--really-quiet", "--no-terminal"]),
        ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
        ("paplay", &[]),
        ("pw-play", &[]),
        ("aplay", &["-q"]),
        ("afplay", &[]),
    ];
    let mut last_error = None;
    for (program, args) in PLAYERS {
        if which(program).is_none() {
            continue;
        }
        let mut command = tokio::process::Command::new(program);
        command
            .args(*args)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        match tokio::time::timeout(Duration::from_secs(60), command.output()).await {
            Ok(Ok(output)) if output.status.success() => return Ok(()),
            Ok(Ok(output)) => {
                tracing::warn!(program, status = %output.status, "音频播放器退出码非 0");
                last_error = Some(format!("{program} 退出码 {}", output.status));
            }
            Ok(Err(err)) => {
                tracing::warn!(%err, program, "音频播放器启动失败");
                last_error = Some(format!("{program} 启动失败: {err}"));
            }
            Err(_) => {
                tracing::warn!(program, "音频播放超时");
                last_error = Some(format!("{program} 播放超时"));
            }
        }
    }
    Err(match last_error {
        Some(detail) => format!("没有可用的音频播放器或播放失败（{detail}）"),
        None => "未找到音频播放器（mpv / ffplay / paplay / pw-play / aplay）".into(),
    })
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

    #[test]
    fn auto_provider_keeps_configured_arguments() {
        let cfg = crate::config::TtsConfig {
            provider: TtsProvider::Auto,
            command: vec!["edge-playback".into(), "--text".into(), "{text}".into()],
            ..Default::default()
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let resolved = runtime.block_on(resolve(&cfg));
        assert_eq!(resolved.label, "command:edge-playback");
        assert_eq!(
            resolved.argv.unwrap(),
            vec!["edge-playback", "--text", "{text}"],
            "自动模式下也要保留用户写的参数"
        );
    }

    #[test]
    fn parses_mimo_audio_and_surfaces_errors() {
        let wav = b"RIFF\x24\x00\x00\x00WAVEfmt ";
        let encoded = base64::engine::general_purpose::STANDARD.encode(wav);
        let value = serde_json::json!({
            "choices": [{ "message": { "audio": { "data": encoded } } }]
        });
        assert_eq!(parse_mimo_audio(&value).unwrap(), wav);

        let error = serde_json::json!({ "error": { "message": "invalid api key" } });
        assert!(
            parse_mimo_audio(&error)
                .unwrap_err()
                .contains("invalid api key")
        );

        let empty = serde_json::json!({ "choices": [] });
        assert!(
            parse_mimo_audio(&empty)
                .unwrap_err()
                .contains("没有音频数据")
        );
    }

    #[test]
    fn resolves_mimo_backend_name() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let configured = TtsConfig {
            provider: TtsProvider::Mimo,
            mimo: MimoConfig {
                api_key: "test-key".into(),
                voice: "冰糖".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(runtime.block_on(resolve_backend(&configured)), "mimo:冰糖");

        let missing = TtsConfig {
            provider: TtsProvider::Mimo,
            ..Default::default()
        };
        assert!(
            runtime
                .block_on(resolve_backend(&missing))
                .contains("api_key")
        );
    }

    #[test]
    fn speak_drops_when_queue_is_full() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let handle = TtsHandle {
            tx,
            pending: Arc::new(AtomicUsize::new(0)),
            limit: Arc::new(AtomicUsize::new(2)),
            dropped: Arc::new(AtomicU64::new(0)),
            backend: Arc::new(RwLock::new("off".into())),
        };
        assert!(handle.speak("chat", "第一条"));
        assert!(handle.speak("chat", "第二条"));
        assert!(!handle.speak("chat", "第三条"), "超过 max_queue 应该丢弃");
        assert_eq!(handle.dropped(), 1);
    }
}
