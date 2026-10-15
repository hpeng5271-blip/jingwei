//! 从进程的输出里认出「它说自己在哪个地址上服务」。
//!
//! ## 为什么不按框架名单去猜
//!
//! 猜的话就得维护一张表：Vite 打 `Local: http://localhost:5173/`、
//! Next 打 `ready - started server on 0.0.0.0:3000`、
//! `python -m http.server` 打 `Serving HTTP on 0.0.0.0 port 8000`、
//! Spring Boot 打 `Tomcat started on port(s): 8080 (http)`。
//! **表里没有的那个**——用户自己写的服务、刚出的新框架、改了名的老框架——
//! 就会被判成「没起来」。
//!
//! 所以这里只认三件事：`host:port`、带协议的 IPv4 地址、以及
//! 「带 port 和"起来了"的意思」的行。**这是进程自己说出来的话**，
//! 跟它叫什么名字无关。
//!
//! ## 「看到地址」与「真的通了」是两件事
//!
//! 这一层只负责**认地址**；连端口是调用方的事（见 `launch.rs` 的等待就绪那段）。
//! 分开的理由很实在：`Error: connect ECONNREFUSED 127.0.0.1:3000` 里也有地址，
//! 只按"出现过地址"判，那种失败会被当成启动成功——
//! **而把失败报成成功，比报成失败糟得多**。

/// 从一行输出里认出「它说自己在哪个地址上服务」。
///
/// ## 为什么不按框架名单去猜
///
/// 猜的话就得维护一张表：Vite 打 `Local: http://localhost:5173/`、
/// Next 打 `ready - started server on 0.0.0.0:3000`、`python -m http.server`
/// 打 `Serving HTTP on 0.0.0.0 port 8000`。**表里没有的那个**——用户自己写的服务、
/// 刚出的新框架、改了名的老框架——就会被判成「没起来」。
///
/// 换成「这一行里有没有 `host:port`」：**这是进程自己说出来的话**，
/// 跟它叫什么名字无关。而「看到地址」与「真的通了」是两件事，
/// 所以调用方还会去连一次那个端口（见等待就绪那一段）。
pub(super) fn serving_url(line: &str) -> Option<(String, u16)> {
    // 先认"一看就是本机"的那几种写法。
    const HOSTS: [&str; 4] = ["localhost:", "127.0.0.1:", "0.0.0.0:", "[::1]:"];
    for h in HOSTS {
        let Some(i) = line.find(h) else { continue };
        let tail = &line[i + h.len()..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        // 端口 0 不是能连的东西；超出 65535 的也不是端口。
        let Ok(p) = digits.parse::<u16>() else { continue };
        if p == 0 {
            continue;
        }
        return Some((format!("{h}{p}"), p));
    }
    // 再认"带协议头的 IPv4 地址"。
    //
    // 为什么不能只认上面那几种：Vite 把地址分两行打，`Local` 是 localhost，
    // `Network` 是局域网地址；而有些框架**只打网络那一行**。
    // 要求它必须带 `http://` 或 `https://` 是为了别把普通字符串（版本号、
    // 时间戳、`a.b.c.d:e` 这种）当成服务地址——那个代价是又一条假就绪。
    ipv4_url(line).or_else(|| port_words(line))
}

/// 从「`Tomcat started on port(s): 8080 (http)`」这种写法里取端口。
///
/// **为什么还要这一条**：Spring Boot 是 Java 那边最常见的东西，而它报的是
/// `port(s): 8080`——没有 `host:port` 那个形状，上面两条规则都抓不到。
///
/// 这条规则的误报空间比前两条大（任何带 "port" 和数字的行都会中），
/// 所以有两道紧箍：**必须还有"起来了"的意思**（started / listening / serving / bound / http），
/// 而且**取到端口之后一定要真连一次**（见等待就绪那段）。两道都在，才敢认。
fn port_words(line: &str) -> Option<(String, u16)> {
    let low = line.to_lowercase();
    if !low.contains("port") {
        return None;
    }
    const UP_WORDS: [&str; 5] = ["started", "listening", "serving", "bound", "http"];
    if !UP_WORDS.iter().any(|w| low.contains(w)) {
        return None;
    }
    // **只看 `port` 之后的数字**。
    //
    // 为什么要限定位置：Spring Boot 的日志行以时间戳开头
    // （`2026-01-01 12:00:00.123 INFO 1234 ---`），取「这一行第一段数字」
    // 会拿到年份 2026——而它落在 u16 范围内，看起来完全像一个合法端口。
    // 这类错的后果很脏：它会去连一个跟服务毫无关系的端口，然后凭「通不通」下结论。
    let chars: Vec<char> = low.chars().collect();
    let at = low.find("port")?;
    let mut i = at + "port".len();
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let digits: String = chars[start..i].iter().collect();
        if let Ok(p) = digits.parse::<u16>() {
            if p > 0 {
                // 证据用**这一行本身**（截断到 60 字），看的人能自己核
                let label = format!("「{}」", line.trim().chars().take(60).collect::<String>());
                return Some((label, p));
            }
        }
        // 这一段数字不合法就继续往后找（`port(s): 0` 这种形状）
    }
    None
}

/// 从 `http://192.168.1.5:5173/` 这种写法里取出 `(192.168.1.5:5173, 5173)`。
fn ipv4_url(line: &str) -> Option<(String, u16)> {
    for scheme in ["http://", "https://"] {
        let Some(i) = line.find(scheme) else { continue };
        let rest = &line[i + scheme.len()..];
        let host: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let parts: Vec<&str> = host.split('.').collect();
        // 四段、每段都在 0..=255 之内，才是一个真的 IPv4
        if parts.len() != 4 || parts.iter().any(|s| s.parse::<u16>().map(|n| n > 255) != Ok(false)) {
            continue;
        }
        let after = &rest[host.len()..];
        let Some(port_part) = after.strip_prefix(':') else {
            continue;
        };
        let digits: String = port_part.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(p) = digits.parse::<u16>() else { continue };
        if p == 0 {
            continue;
        }
        return Some((format!("{host}:{p}"), p));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 各家框架报出来的监听地址都认() {
        for (line, port) in [
            ("  ➜  Local:   http://localhost:5173/", 5173u16),
            (
                "ready - started server on 0.0.0.0:3000, url: http://localhost:3000",
                3000,
            ),
            (
                "Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ...",
                8000,
            ),
            ("  - Network: http://192.168.1.5:5173/", 5173),
        ] {
            assert_eq!(serving_url(line).map(|(_, p)| p), Some(port), "{line}");
        }
    }

    #[test]
    fn 连不上的报错里那个地址也认得出来() {
        let line = "Error: connect ECONNREFUSED 127.0.0.1:3000";
        assert_eq!(
            serving_url(line),
            Some(("127.0.0.1:3000".to_string(), 3000)),
            "认出来之后由调用方去连——连不上就不算就绪"
        );
    }

    #[test]
    fn 没有地址的行不会瞎认() {
        assert!(serving_url("vite v5.2.0 building for production...").is_none());
        assert!(serving_url("localhost 是不可用的").is_none(), "没有端口号");
        assert!(
            serving_url("connecting to localhost:0").is_none(),
            "端口 0 不是端口"
        );
        assert!(
            serving_url("see http://localhost:99999/").is_none(),
            "超出 65535"
        );
    }

    #[test]
    fn spring_boot_那种_port_写法也认() {
        let line = "2026-01-01 12:00:00.123  INFO 1234 --- [main] o.s.b.w.embedded.tomcat.TomcatWebServer  : Tomcat started on port(s): 8080 (http) with context path ''";
        assert_eq!(serving_url(line).map(|(_, p)| p), Some(8080));
        assert_eq!(
            serving_url("Tomcat started on port(s): 8080 (http)").map(|(_, p)| p),
            Some(8080)
        );
    }

    #[test]
    fn 只有_port_和数字不算服务地址() {
        assert!(
            serving_url("importing 8080 modules from cache").is_none(),
            "没有 started/listening/serving/bound/http 这些意思"
        );
        assert!(serving_url("failed to configure port 8080").is_none());
    }
}
