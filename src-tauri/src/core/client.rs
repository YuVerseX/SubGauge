use chrono::Utc;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use url::Url;

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64,
}
pub struct Api {
    client: Client,
}
#[derive(Clone, Debug)]
pub struct ApiError {
    pub message: String,
    pub unauthorized: bool,
    pub retry_after: Option<u64>,
}
impl From<String> for ApiError {
    fn from(message: String) -> Self {
        Self {
            message,
            unauthorized: false,
            retry_after: None,
        }
    }
}
impl From<&str> for ApiError {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}
pub fn normalize_site(input: &str) -> Result<String, String> {
    let mut url =
        Url::parse(input.trim()).map_err(|_| "请输入完整站点地址，例如 https://sub.example.com")?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("站点地址不能包含凭据、查询参数或片段。".into());
    }
    if url.scheme() == "http"
        && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
    {
        return Err("请使用 HTTPS 保护登录凭据；仅本机地址允许 HTTP。".into());
    }
    let path = url
        .path()
        .trim_end_matches('/')
        .trim_end_matches("/api/v1")
        .trim_end_matches('/')
        .to_owned();
    url.set_path(&path);
    Ok(url.to_string().trim_end_matches('/').to_owned())
}
impl Api {
    pub fn new() -> Result<Self, String> {
        let builder = Client::builder();
        // Loopback fixtures must not depend on the developer's proxy configuration.
        #[cfg(test)]
        let builder = builder.no_proxy();
        Ok(Self {
            client: builder
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(25))
                .connect_timeout(Duration::from_secs(10))
                .user_agent("SubGauge/0.1")
                .build()
                .map_err(|_| "无法初始化网络服务")?,
        })
    }
    pub async fn raw(
        &self,
        site: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        query: &[(String, String)],
    ) -> Result<Value, ApiError> {
        let url = format!("{site}/api/v1{path}");
        let mut req = if body.is_some() {
            self.client.post(&url)
        } else {
            self.client.get(&url)
        };
        req = req.query(query);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let mut response = req.send().await.map_err(|e| {
            if e.is_timeout() {
                ApiError::from("连接超时，保留上次结果。")
            } else {
                ApiError::from("无法连接站点，请检查地址、网络和证书。")
            }
        })?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|v| v.clamp(1, 3600));
        if status.is_redirection() {
            return Err("站点返回重定向，为保护凭据已停止请求，请填写最终站点地址。".into());
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(ApiError {
                message: "站点限流，稍后自动重试。".into(),
                unauthorized: false,
                retry_after: Some(retry_after.unwrap_or(60)),
            });
        }
        // Never expose arbitrary server response strings: they may echo submitted credentials.
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ApiError::from("读取站点响应失败。"))?
        {
            if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
                return Err("站点响应过大。".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let v: Value = serde_json::from_slice(&bytes).map_err(|_| ApiError {
            message: if status == StatusCode::UNAUTHORIZED {
                "登录已过期，请重新登录。"
            } else {
                "站点未返回 Sub2API JSON 数据，请检查站点地址。"
            }
            .into(),
            unauthorized: status == StatusCode::UNAUTHORIZED,
            retry_after: None,
        })?;
        let code = v.get("code").and_then(Value::as_i64).unwrap_or(0);
        if !status.is_success() || code != 0 {
            let hint = format!(
                "{} {}",
                v["message"].as_str().unwrap_or(""),
                v["reason"].as_str().unwrap_or("")
            )
            .to_lowercase();
            let message = if hint.contains("captcha") || hint.contains("turnstile") {
                "站点要求验证码验证；当前连接方式暂不支持此挑战，需要验证该站点支持的登录衔接方式。"
            } else if status == StatusCode::UNAUTHORIZED {
                "登录失效或邮箱密码不正确，请重新登录。"
            } else if status == StatusCode::FORBIDDEN {
                "站点拒绝访问当前个人接口。"
            } else if status == StatusCode::NOT_FOUND {
                "站点缺少所需个人接口，可能需要更新 Sub2API。"
            } else if path.contains("2fa") {
                "二次验证码无效或已过期，请重试。"
            } else {
                "站点请求失败，请稍后重试。"
            };
            return Err(ApiError {
                message: message.into(),
                unauthorized: status == StatusCode::UNAUTHORIZED,
                retry_after: None,
            });
        }
        Ok(v.get("data").cloned().unwrap_or(v))
    }
    pub fn session(v: &Value) -> Result<Session, String> {
        let token = v["access_token"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("站点未返回有效会话")?;
        Ok(Session {
            access_token: token.into(),
            refresh_token: v["refresh_token"]
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned),
            expires_at: Utc::now().timestamp()
                + v["expires_in"]
                    .as_i64()
                    .unwrap_or(3600)
                    .clamp(1, 31_536_000),
        })
    }
    async fn renew(&self, site: &str, session: &mut Session) -> Result<(), ApiError> {
        let refresh = session.refresh_token.as_ref().ok_or(ApiError {
            message: "登录已过期，请重新登录。".into(),
            unauthorized: true,
            retry_after: None,
        })?;
        let v = self
            .raw(
                site,
                "/auth/refresh",
                None,
                Some(json!({"refresh_token":refresh})),
                &[],
            )
            .await?;
        let mut next = Self::session(&v).map_err(ApiError::from)?;
        if next.refresh_token.is_none() {
            next.refresh_token = session.refresh_token.clone();
        }
        *session = next;
        Ok(())
    }
    /// Call while holding the account's session mutex. A single refresh serves all waiters.
    pub async fn request(
        &self,
        site: &str,
        path: &str,
        session: &mut Session,
        query: &[(String, String)],
    ) -> Result<Value, ApiError> {
        if session.expires_at <= Utc::now().timestamp() + 30 {
            self.renew(site, session).await?;
        }
        let result = self
            .raw(site, path, Some(&session.access_token), None, query)
            .await;
        match result {
            Err(e) if e.unauthorized => {
                self.renew(site, session).await?;
                self.raw(site, path, Some(&session.access_token), None, query)
                    .await
            }
            other => other,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_site_normalization() {
        assert_eq!(
            normalize_site("https://EXAMPLE.com/api/v1/").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            normalize_site("https://example.com/sub/").unwrap(),
            "https://example.com/sub"
        );
        assert!(normalize_site("https://secret@example.com").is_err());
        assert!(normalize_site("https://example.com/?token=x").is_err());
        assert!(normalize_site("http://example.com").is_err());
    }
}
