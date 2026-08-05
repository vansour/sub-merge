use proptest::prelude::*;
use proxy_core::parser::parse_line;

proptest! {
    #[test]
    fn parse_line_never_panics(s in ".*") {
        // 任意字符串输入，解析器必须返回 Result 而非 panic
        let _ = parse_line(&s);
    }

    #[test]
    fn parse_line_base64_mutations(base in "[a-zA-Z0-9+/=_-]{0,200}") {
        let _ = parse_line(&format!("vmess://{}", base));
        let _ = parse_line(&format!("ssr://{}", base));
        let _ = parse_line(&format!("ss://{}", base));
    }
}
