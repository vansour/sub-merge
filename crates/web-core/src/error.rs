// API 请求错误类型：区分 HTTP 状态码与网络/解析错误。
// status = Some(401) 表示鉴权失败（token 失效），调用方据此决定是否清除本地 token；
// 网络错误/5xx 等瞬时错误 status 为 None 或非 401，不得清除 token。
#[derive(Debug)]
pub struct ApiError {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<ApiError> for String {
    fn from(e: ApiError) -> String {
        e.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_prints_message() {
        let e = ApiError {
            status: Some(401),
            message: "HTTP 401: unauthorized".into(),
        };
        assert_eq!(e.to_string(), "HTTP 401: unauthorized");
    }

    #[test]
    fn display_without_status() {
        let e = ApiError {
            status: None,
            message: "network down".into(),
        };
        assert_eq!(e.to_string(), "network down");
    }

    #[test]
    fn into_string_converts() {
        let e = ApiError {
            status: None,
            message: "boom".into(),
        };
        let s: String = e.into();
        assert_eq!(s, "boom");
    }

    #[test]
    fn status_field_semantics() {
        // 401 与瞬时错误（None/5xx）的区分由调用方决定，这里锁定字段语义防回归。
        assert_eq!(
            ApiError {
                status: Some(401),
                message: "".into()
            }
            .status,
            Some(401)
        );
        assert_eq!(
            ApiError {
                status: Some(500),
                message: "".into()
            }
            .status,
            Some(500)
        );
        assert_eq!(
            ApiError {
                status: None,
                message: "".into()
            }
            .status,
            None
        );
    }
}
