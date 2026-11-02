//! 回归测试：对外请求**不能**吃系统代理。

use std::io::{Read, Write};
use std::net::TcpListener;

/// 起一个只回一次 HTTP 的迷你服务器，返回它的地址。
fn spawn_json_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let body = r#"{"code":0,"ok":true}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://{addr}/ping")
}

#[test]
fn outbound_requests_ignore_system_proxy() {
    // 把系统代理指向一个没人监听的端口：客户端要是还读 http_proxy，
    // 这个请求必然连接失败。弹幕姬必须直连，否则用户切代理就会断流。
    //
    // SAFETY: 这个集成测试是独立进程里唯一的测试，改环境变量不会影响别人。
    unsafe {
        std::env::set_var("http_proxy", "http://127.0.0.1:1");
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");
        std::env::set_var("all_proxy", "http://127.0.0.1:1");
        std::env::set_var("ALL_PROXY", "http://127.0.0.1:1");
        std::env::remove_var("MAKUHIME_SYSTEM_PROXY");
    }

    let url = spawn_json_server();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let value = runtime.block_on(async move {
        let client = makuhime_core::http::HttpClient::new().unwrap();
        client.get_json(&url).await
    });
    let value = value.expect("请求不该被系统代理劫持");
    assert_eq!(value.get("ok").and_then(|v| v.as_bool()), Some(true));
}
